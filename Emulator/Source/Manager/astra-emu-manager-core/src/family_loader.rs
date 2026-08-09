use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

use abi_stable::{
    library::{AbiHeaderRef, ROOT_MODULE_LOADER_NAME_WITH_NUL},
    std_types::{ROption, RResult, RString, RVec},
};
use astra_core::Hash256;
use astra_emu_family_api::*;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use libloading::Library;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

static HOST_SERVICES: OnceLock<Mutex<BTreeMap<String, DynamicFamilyHostServicesV8>>> =
    OnceLock::new();

#[derive(Clone)]
pub struct DynamicFamilyHostServicesV8 {
    pub vfs: Arc<dyn LegacyVfsReader>,
    pub text_layout: Option<Arc<dyn LegacyTextLayoutHostV8>>,
    pub private_material: Option<Arc<dyn LegacyPrivateMaterialHostV8>>,
    pub save_store: Option<Arc<dyn LegacySaveStoreHostV8>>,
}

impl DynamicFamilyHostServicesV8 {
    pub fn vfs_only(vfs: Arc<dyn LegacyVfsReader>) -> Self {
        Self {
            vfs,
            text_layout: None,
            private_material: None,
            save_store: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FamilyPluginManifest {
    pub schema: String,
    pub family_id: String,
    pub plugin_id: String,
    pub provider_id: String,
    pub engine_version: String,
    pub rustc_fingerprint: String,
    pub feature_fingerprint: String,
    pub abi_fingerprint: String,
    pub binary_hash: Hash256,
    pub signer_identity: String,
    pub signature_algorithm: String,
    pub signature_hex: String,
    pub package_eligible: bool,
    pub supported_targets: Vec<String>,
    pub native_manifest_hash: Option<Hash256>,
}

#[derive(Serialize)]
struct SignedFamilyPluginIdentity<'a> {
    schema: &'a str,
    family_id: &'a str,
    plugin_id: &'a str,
    provider_id: &'a str,
    engine_version: &'a str,
    rustc_fingerprint: &'a str,
    feature_fingerprint: &'a str,
    abi_fingerprint: &'a str,
    binary_hash: Hash256,
    signer_identity: &'a str,
    package_eligible: bool,
    supported_targets: &'a [String],
    native_manifest_hash: Option<Hash256>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyPluginGate {
    pub engine_version: String,
    pub rustc_fingerprint: String,
    pub feature_fingerprint: String,
    pub abi_fingerprint: String,
    pub target: String,
    pub allowed_signers: BTreeSet<String>,
    pub require_native_manifest_binding: bool,
    pub expected_native_manifest_hash: Option<Hash256>,
}

pub trait FamilySignatureVerifier: Send + Sync {
    fn verify_official_signature(
        &self,
        binary: &[u8],
        manifest: &FamilyPluginManifest,
    ) -> Result<(), FamilyPluginLoadError>;
}

pub struct Ed25519FamilySignatureVerifier {
    trust_roots: BTreeMap<String, VerifyingKey>,
}

impl Ed25519FamilySignatureVerifier {
    pub fn new(
        roots: impl IntoIterator<Item = (String, [u8; 32])>,
    ) -> Result<Self, FamilyPluginLoadError> {
        let mut trust_roots = BTreeMap::new();
        for (identity, bytes) in roots {
            validate_symbol("signer_identity", &identity)
                .map_err(|error| FamilyPluginLoadError::Signature(error.to_string()))?;
            let key = VerifyingKey::from_bytes(&bytes)
                .map_err(|_| FamilyPluginLoadError::Signature("invalid trust root".into()))?;
            if trust_roots.insert(identity, key).is_some() {
                return Err(FamilyPluginLoadError::Signature(
                    "duplicate signer trust root".into(),
                ));
            }
        }
        if trust_roots.is_empty() {
            return Err(FamilyPluginLoadError::Signature(
                "no official signer trust root is configured".into(),
            ));
        }
        Ok(Self { trust_roots })
    }

    pub fn verify_manifest_identity(
        &self,
        manifest: &FamilyPluginManifest,
    ) -> Result<(), FamilyPluginLoadError> {
        if manifest.signature_algorithm != "ed25519-v1" {
            return Err(FamilyPluginLoadError::Signature(
                "signature algorithm mismatch".into(),
            ));
        }
        let signature_bytes = hex::decode(&manifest.signature_hex).map_err(|_| {
            FamilyPluginLoadError::Signature("signature encoding is invalid".into())
        })?;
        let signature_bytes: [u8; 64] = signature_bytes
            .try_into()
            .map_err(|_| FamilyPluginLoadError::Signature("signature length is invalid".into()))?;
        let signature = Signature::from_bytes(&signature_bytes);
        let key = self
            .trust_roots
            .get(&manifest.signer_identity)
            .ok_or_else(|| FamilyPluginLoadError::Signature("signer is not trusted".into()))?;
        let payload = canonical_family_manifest_bytes(manifest)?;
        key.verify(&payload, &signature)
            .map_err(|_| FamilyPluginLoadError::Signature("signature verification failed".into()))
    }
}

impl FamilySignatureVerifier for Ed25519FamilySignatureVerifier {
    fn verify_official_signature(
        &self,
        binary: &[u8],
        manifest: &FamilyPluginManifest,
    ) -> Result<(), FamilyPluginLoadError> {
        if Hash256::from_sha256(binary) != manifest.binary_hash {
            return Err(FamilyPluginLoadError::Signature(
                "binary binding mismatch".into(),
            ));
        }
        self.verify_manifest_identity(manifest)
    }
}

pub struct PinnedStaticFamilyVerifier {
    signature_verifier: Ed25519FamilySignatureVerifier,
    binary_hash: Hash256,
}

impl PinnedStaticFamilyVerifier {
    pub fn new(signature_verifier: Ed25519FamilySignatureVerifier, binary_hash: Hash256) -> Self {
        Self {
            signature_verifier,
            binary_hash,
        }
    }
}

impl StaticFamilyRegistrationVerifier for PinnedStaticFamilyVerifier {
    fn verify_static_registration(
        &self,
        manifest: &FamilyPluginManifest,
    ) -> Result<(), FamilyPluginLoadError> {
        if manifest.binary_hash != self.binary_hash {
            return Err(FamilyPluginLoadError::Signature(
                "static archive binding mismatch".into(),
            ));
        }
        self.signature_verifier.verify_manifest_identity(manifest)
    }
}

pub trait StaticFamilyRegistrationVerifier: Send + Sync {
    fn verify_static_registration(
        &self,
        manifest: &FamilyPluginManifest,
    ) -> Result<(), FamilyPluginLoadError>;
}

pub type StaticFamilyFactory =
    fn(Arc<dyn LegacyVfsReader>) -> Result<Box<dyn LegacyRuntimeProvider>, LegacyProviderError>;

#[derive(Clone)]
pub struct StaticFamilyRegistration {
    pub manifest: FamilyPluginManifest,
    pub factory: StaticFamilyFactory,
}

pub struct StaticFamilyRegistry {
    gate: FamilyPluginGate,
    verifier: Arc<dyn StaticFamilyRegistrationVerifier>,
    registrations: BTreeMap<String, StaticFamilyRegistration>,
}

impl StaticFamilyRegistry {
    pub fn new(
        gate: FamilyPluginGate,
        verifier: Arc<dyn StaticFamilyRegistrationVerifier>,
    ) -> Self {
        Self {
            gate,
            verifier,
            registrations: BTreeMap::new(),
        }
    }

    pub fn register(
        &mut self,
        registration: StaticFamilyRegistration,
    ) -> Result<(), FamilyPluginLoadError> {
        validate_manifest(&registration.manifest, &self.gate)?;
        self.verifier
            .verify_static_registration(&registration.manifest)?;
        if self
            .registrations
            .contains_key(&registration.manifest.family_id)
        {
            return Err(FamilyPluginLoadError::Manifest(
                "duplicate static family id".into(),
            ));
        }
        self.registrations
            .insert(registration.manifest.family_id.clone(), registration);
        Ok(())
    }

    pub fn create(
        &self,
        family_id: &str,
        vfs: Arc<dyn LegacyVfsReader>,
    ) -> Result<Box<dyn LegacyRuntimeProvider>, FamilyPluginLoadError> {
        let registration = self.registrations.get(family_id).ok_or_else(|| {
            FamilyPluginLoadError::Manifest("explicit static family binding is missing".into())
        })?;
        let provider = (registration.factory)(vfs).map_err(provider_error)?;
        let descriptor = provider.descriptor();
        descriptor.validate().map_err(provider_error)?;
        validate_descriptor_binding(&registration.manifest, &descriptor)?;
        Ok(provider)
    }
}

#[derive(Debug, Error)]
pub enum FamilyPluginLoadError {
    #[error("ASTRA_EMU_FAMILY_MANIFEST: {0}")]
    Manifest(String),
    #[error("ASTRA_EMU_FAMILY_BINARY_READ")]
    BinaryRead,
    #[error("ASTRA_EMU_FAMILY_SIGNATURE: {0}")]
    Signature(String),
    #[error("ASTRA_EMU_FAMILY_ABI_LOAD:{0}")]
    AbiLoad(&'static str),
    #[error("ASTRA_EMU_FAMILY_PROVIDER: {0}")]
    Provider(String),
}

pub struct DynamicFamilyLoader {
    gate: FamilyPluginGate,
    signature_verifier: Arc<dyn FamilySignatureVerifier>,
}

impl DynamicFamilyLoader {
    pub fn new(
        gate: FamilyPluginGate,
        signature_verifier: Arc<dyn FamilySignatureVerifier>,
    ) -> Self {
        Self {
            gate,
            signature_verifier,
        }
    }

    pub fn load(
        &self,
        path: impl AsRef<Path>,
        manifest: FamilyPluginManifest,
        instance_id: String,
        services: DynamicFamilyHostServicesV8,
    ) -> Result<DynamicLegacyRuntimeProvider, FamilyPluginLoadError> {
        validate_manifest(&manifest, &self.gate)?;
        validate_symbol("instance_id", &instance_id)
            .map_err(|error| FamilyPluginLoadError::Manifest(error.to_string()))?;
        let binary = fs::read(path.as_ref()).map_err(|_| FamilyPluginLoadError::BinaryRead)?;
        if Hash256::from_sha256(&binary) != manifest.binary_hash {
            return Err(FamilyPluginLoadError::Manifest(
                "binary hash mismatch".into(),
            ));
        }
        self.signature_verifier
            .verify_official_signature(&binary, &manifest)?;
        let library = Arc::new(
            unsafe { Library::new(path.as_ref()) }
                .map_err(|_| FamilyPluginLoadError::AbiLoad("library"))?,
        );
        let module = unsafe { root_module(&library)? };
        let descriptor: LegacyFamilyPluginDescriptor =
            native_result((module.descriptor())()).map_err(provider_error)?;
        descriptor.validate().map_err(provider_error)?;
        validate_descriptor_binding(&manifest, &descriptor)?;

        let host_token = format!("emu.vfs.{}", Hash256::from_sha256(instance_id.as_bytes()));
        let mut registry = host_services()
            .lock()
            .map_err(|_| FamilyPluginLoadError::AbiLoad("host_service_registry"))?;
        if registry.insert(host_token.clone(), services).is_some() {
            return Err(FamilyPluginLoadError::Manifest(
                "host service token collision".into(),
            ));
        }
        drop(registry);
        let request = FfiProviderInstanceRequest {
            instance_id: instance_id.clone().into(),
        };
        let services = FfiLegacyHostServices {
            host_token: host_token.clone().into(),
            stat_vfs: ffi_stat_vfs,
            read_vfs_range: ffi_read_vfs_range,
            enumerate_vfs: ffi_enumerate_vfs,
            layout_text: ffi_layout_text,
            release_text_layout: ffi_release_text_layout,
            read_private_material: ffi_read_private_material,
            list_save_slots: ffi_list_save_slots,
            read_save_slot: ffi_read_save_slot,
            atomic_write_save_slot: ffi_atomic_write_save_slot,
        };
        if let Err(error) = native_result::<_, ()>((module.create_instance())(services, request)) {
            remove_host_services(&host_token);
            return Err(provider_error(error));
        }
        Ok(DynamicLegacyRuntimeProvider {
            descriptor,
            instance_id,
            host_token,
            sessions: BTreeMap::new(),
            module,
            _library: library,
        })
    }
}

pub struct DynamicLegacyRuntimeProvider {
    descriptor: LegacyFamilyPluginDescriptor,
    instance_id: String,
    host_token: String,
    sessions: BTreeMap<String, LegacyRuntimeHostCtx>,
    module: AstraLegacyFamilyModuleRef,
    _library: Arc<Library>,
}

impl LegacyRuntimeProvider for DynamicLegacyRuntimeProvider {
    fn descriptor(&self) -> LegacyFamilyPluginDescriptor {
        self.descriptor.clone()
    }

    fn probe(
        &self,
        ctx: &LegacyRuntimeHostCtx,
        request: LegacyProbeRequest,
    ) -> Result<LegacyProbeReport, LegacyProviderError> {
        native_result((self.module.probe())(FfiProbeCall {
            instance_id: self.instance_id.clone().into(),
            ctx: ctx.clone().into(),
            request: request.into(),
        }))
    }

    fn open(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        request: LegacyOpenRequest,
    ) -> Result<LegacyRuntimeSessionId, LegacyProviderError> {
        let result = LegacyRuntimeSessionId(native_result::<_, String>((self.module.open())(
            FfiOpenCall {
                instance_id: self.instance_id.clone().into(),
                ctx: ctx.clone().into(),
                request: request.into(),
            },
        ))?);
        if self
            .sessions
            .insert(result.0.clone(), ctx.clone())
            .is_some()
        {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_FFI_SESSION_DUPLICATE",
                "dynamic provider returned a duplicate session id",
            ));
        }
        Ok(result)
    }

    fn step(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
        input: LegacyStepInput,
    ) -> Result<LegacyStepOutput, LegacyProviderError> {
        self.validate_session(ctx, session)?;
        match (self.module.step())(FfiStepCall {
            instance_id: self.instance_id.clone().into(),
            ctx: ctx.clone().into(),
            session_id: session.0.clone().into(),
            input: input.into(),
        }) {
            RResult::ROk(output) => output.try_into(),
            RResult::RErr(error) => Err(error.into()),
        }
    }

    fn save(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
    ) -> Result<LegacySnapshotEnvelope, LegacyProviderError> {
        self.validate_session(ctx, session)?;
        native_result((self.module.save())(self.session_call(ctx, session)))
    }

    fn restore(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
        snapshot: &LegacySnapshotEnvelope,
    ) -> Result<LegacyRestoreReport, LegacyProviderError> {
        self.validate_session(ctx, session)?;
        native_result((self.module.restore())(FfiRestoreCall {
            instance_id: self.instance_id.clone().into(),
            ctx: ctx.clone().into(),
            session_id: session.0.clone().into(),
            snapshot: snapshot.clone().into(),
        }))
    }

    fn shutdown(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
    ) -> Result<LegacyShutdownReport, LegacyProviderError> {
        self.validate_session(ctx, session)?;
        let report = native_result((self.module.shutdown())(self.session_call(ctx, session)))?;
        self.sessions.remove(&session.0);
        Ok(report)
    }

    fn take_ephemeral_text(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
        lease_id: &str,
    ) -> Result<Option<LegacyEphemeralText>, LegacyProviderError> {
        self.validate_session(ctx, session)?;
        validate_symbol("text_lease_id", lease_id)?;
        let value =
            native_result::<_, ROption<FfiEphemeralText>>((self.module.take_ephemeral_text())(
                FfiTextLeaseCall {
                    instance_id: self.instance_id.clone().into(),
                    ctx: ctx.clone().into(),
                    session_id: session.0.clone().into(),
                    lease_id: lease_id.into(),
                },
            ))?;
        Ok(value.into_option().map(Into::into))
    }

    fn read_session_resource(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
        resource_uri: &str,
        max_bytes: u64,
    ) -> Result<astra_byte_source::OwnedByteBuffer, LegacyProviderError> {
        self.validate_session(ctx, session)?;
        let bytes = match (self.module.read_session_resource())(FfiResourceReadCall {
            instance_id: self.instance_id.clone().into(),
            ctx: ctx.clone().into(),
            session_id: session.0.clone().into(),
            resource_uri: resource_uri.into(),
            max_bytes,
        }) {
            RResult::ROk(bytes) => bytes,
            RResult::RErr(error) => return Err(error.into()),
        };
        if bytes.len() as u64 > max_bytes {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_FFI_RESOURCE_BOUNDS",
                "family resource exceeds the requested byte bound",
            ));
        }
        Ok(bytes.into_owned())
    }

    fn begin_session_resource_read(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
        resource_uri: &str,
        max_bytes: u64,
    ) -> Result<LegacyResourceRead, LegacyProviderError> {
        self.validate_session(ctx, session)?;
        let module = self.module;
        let _library = Arc::clone(&self._library);
        let call = FfiResourceReadCall {
            instance_id: self.instance_id.clone().into(),
            ctx: ctx.clone().into(),
            session_id: session.0.clone().into(),
            resource_uri: resource_uri.into(),
            max_bytes,
        };
        LegacyResourceRead::spawn(move || {
            let _library = _library;
            let bytes = match (module.read_session_resource())(call) {
                RResult::ROk(bytes) => bytes,
                RResult::RErr(error) => return Err(error.into()),
            };
            if bytes.len() as u64 > max_bytes {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_FFI_RESOURCE_BOUNDS",
                    "family resource exceeds the requested byte bound",
                ));
            }
            Ok(bytes.into_owned())
        })
    }
}

impl DynamicLegacyRuntimeProvider {
    fn session_call(
        &self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
    ) -> FfiSessionCall {
        FfiSessionCall {
            instance_id: self.instance_id.clone().into(),
            ctx: ctx.clone().into(),
            session_id: session.0.clone().into(),
        }
    }

    fn validate_session(
        &self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
    ) -> Result<(), LegacyProviderError> {
        match self.sessions.get(&session.0) {
            Some(bound) if bound == ctx => Ok(()),
            Some(_) => Err(LegacyProviderError::invalid(
                "ASTRA_EMU_FFI_SESSION_CONTEXT",
                "session host context changed",
            )),
            None => Err(LegacyProviderError::invalid(
                "ASTRA_EMU_FFI_SESSION_MISSING",
                "session id is not active",
            )),
        }
    }
}

impl Drop for DynamicLegacyRuntimeProvider {
    fn drop(&mut self) {
        for (session_id, ctx) in std::mem::take(&mut self.sessions) {
            let _ = (self.module.shutdown())(FfiSessionCall {
                instance_id: self.instance_id.clone().into(),
                ctx: ctx.into(),
                session_id: session_id.into(),
            });
        }
        let _ = (self.module.destroy_instance())(FfiProviderInstanceRequest {
            instance_id: self.instance_id.clone().into(),
        });
        remove_host_services(&self.host_token);
    }
}

extern "C" fn ffi_stat_vfs(
    host_token: RString,
    call: FfiVfsStatCall,
) -> FfiLegacyResult<FfiByteSourceStat> {
    let result = (|| {
        let registry = host_services().lock().map_err(|_| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_LOCK_POISONED",
                "host VFS registry lock is poisoned",
            )
        })?;
        let services = registry.get(host_token.as_str()).ok_or_else(|| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_HOST_TOKEN", "host VFS token is not active")
        })?;
        services
            .vfs
            .stat_file(call.mount_set_id.as_str(), call.uri.as_str())
    })();
    ffi_result(result)
}

extern "C" fn ffi_read_vfs_range(
    host_token: RString,
    call: FfiVfsRangeCall,
) -> FfiLegacyResult<FfiRangeReadResult> {
    let result = (|| {
        let registry = host_services().lock().map_err(|_| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_LOCK_POISONED",
                "host VFS registry lock is poisoned",
            )
        })?;
        let services = registry.get(host_token.as_str()).ok_or_else(|| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_HOST_TOKEN", "host VFS token is not active")
        })?;
        services.vfs.read_file_range(
            call.mount_set_id.as_str(),
            call.uri.as_str(),
            astra_byte_source::SourceRevision(call.expected_revision),
            call.range.into(),
            call.max_bytes,
        )
    })();
    ffi_result(result)
}

extern "C" fn ffi_enumerate_vfs(
    host_token: RString,
    call: FfiVfsEnumerateCall,
) -> FfiLegacyResult<RVec<FfiVfsListedFile>> {
    let result = (|| {
        let registry = host_services().lock().map_err(|_| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_LOCK_POISONED",
                "host VFS registry lock is poisoned",
            )
        })?;
        let services = registry.get(host_token.as_str()).ok_or_else(|| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_HOST_TOKEN", "host VFS token is not active")
        })?;
        services
            .vfs
            .enumerate_by_extension(
                call.mount_set_id.as_str(),
                call.root.as_str(),
                call.extension_without_dot.as_str(),
                call.max_entries,
            )
            .map(|entries| {
                entries
                    .into_iter()
                    .map(FfiVfsListedFile::from)
                    .collect::<Vec<_>>()
                    .into()
            })
    })();
    ffi_result::<RVec<FfiVfsListedFile>, RVec<FfiVfsListedFile>>(result)
}

fn required_host_service<T>(
    service: Option<Arc<T>>,
    name: &'static str,
) -> Result<Arc<T>, LegacyProviderError>
where
    T: ?Sized,
{
    service.ok_or_else(|| {
        LegacyProviderError::invalid(
            "ASTRA_EMU_V8_HOST_SERVICE_UNBOUND",
            format!("required Family ABI v8 host service is not bound: {name}"),
        )
    })
}

extern "C" fn ffi_layout_text(
    host_token: RString,
    call: FfiTextLayoutCallV8,
) -> FfiLegacyResult<FfiTextLayoutResultV8> {
    let result: Result<FfiTextLayoutResultV8, LegacyProviderError> = (|| {
        let service = required_host_service(
            get_host_services(host_token.as_str())?.text_layout,
            "text_layout",
        )?;
        let session = LegacyRuntimeSessionId(call.session_id.to_string());
        let result = service.layout(&session, call.request.into())?;
        result.validate()?;
        Ok(result.into())
    })();
    ffi_result(result)
}

extern "C" fn ffi_release_text_layout(
    host_token: RString,
    call: FfiTextLayoutReleaseCallV8,
) -> FfiLegacyResult<()> {
    let result = (|| {
        let service = required_host_service(
            get_host_services(host_token.as_str())?.text_layout,
            "text_layout",
        )?;
        service.release_layout(
            &LegacyRuntimeSessionId(call.session_id.to_string()),
            call.layout_token.as_str(),
        )
    })();
    ffi_result(result)
}

extern "C" fn ffi_read_private_material(
    host_token: RString,
    call: FfiPrivateMaterialCallV8,
) -> FfiLegacyResult<FfiSecretBufferV8> {
    let result = (|| {
        let service = required_host_service(
            get_host_services(host_token.as_str())?.private_material,
            "private_material",
        )?;
        let secret = service.read_private_material(
            &LegacyRuntimeSessionId(call.session_id.to_string()),
            LegacyPrivateMaterialRequestV8 {
                secret_id: call.secret_id.to_string(),
                exact_len: call.exact_len,
            },
        )?;
        if secret.len() != call.exact_len as usize {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_PRIVATE_MATERIAL_LENGTH",
                "private material host returned an unexpected length",
            ));
        }
        Ok(FfiSecretBufferV8 {
            bytes: secret.as_slice().to_vec().into(),
        })
    })();
    ffi_result(result)
}

extern "C" fn ffi_list_save_slots(
    host_token: RString,
    call: FfiSaveListCallV8,
) -> FfiLegacyResult<RVec<FfiSaveSlotV8>> {
    let result: Result<RVec<FfiSaveSlotV8>, LegacyProviderError> = (|| {
        let service = required_host_service(
            get_host_services(host_token.as_str())?.save_store,
            "save_store",
        )?;
        let slots = service.list_slots(
            &LegacyRuntimeSessionId(call.session_id.to_string()),
            call.max_slots,
        )?;
        Ok(slots
            .into_iter()
            .map(FfiSaveSlotV8::from)
            .collect::<Vec<_>>()
            .into())
    })();
    ffi_result(result)
}

extern "C" fn ffi_read_save_slot(
    host_token: RString,
    call: FfiSaveReadCallV8,
) -> FfiLegacyResult<FfiSaveReadResultV8> {
    let result: Result<FfiSaveReadResultV8, LegacyProviderError> = (|| {
        let service = required_host_service(
            get_host_services(host_token.as_str())?.save_store,
            "save_store",
        )?;
        let value = service.read_slot(
            &LegacyRuntimeSessionId(call.session_id.to_string()),
            call.slot_id.as_str(),
            call.expected_revision.into_option(),
            call.max_bytes,
        )?;
        if value.payload.len() as u64 > call.max_bytes {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_SAVE_READ_BUDGET",
                "save store returned a payload above the requested bound",
            ));
        }
        Ok(FfiSaveReadResultV8 {
            slot_id: value.slot_id.into(),
            revision: value.revision,
            payload: value.payload.into_ffi(),
        })
    })();
    ffi_result(result)
}

extern "C" fn ffi_atomic_write_save_slot(
    host_token: RString,
    call: FfiSaveWriteCallV8,
) -> FfiLegacyResult<FfiSaveWriteResultV8> {
    let result: Result<FfiSaveWriteResultV8, LegacyProviderError> = (|| {
        if call.payload.len() as u64 > call.max_bytes {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_SAVE_WRITE_BUDGET",
                "save write payload exceeds the declared bound",
            ));
        }
        let service = required_host_service(
            get_host_services(host_token.as_str())?.save_store,
            "save_store",
        )?;
        service
            .atomic_write_slot(
                &LegacyRuntimeSessionId(call.session_id.to_string()),
                LegacySaveWriteRequestV8 {
                    slot_id: call.slot_id.to_string(),
                    expected_revision: call.expected_revision.into_option(),
                    max_bytes: call.max_bytes,
                    payload: call.payload.into_owned(),
                },
            )
            .map(FfiSaveWriteResultV8::from)
    })();
    ffi_result(result)
}

fn validate_manifest(
    manifest: &FamilyPluginManifest,
    gate: &FamilyPluginGate,
) -> Result<(), FamilyPluginLoadError> {
    for (field, value) in [
        ("family_id", manifest.family_id.as_str()),
        ("plugin_id", manifest.plugin_id.as_str()),
        ("provider_id", manifest.provider_id.as_str()),
        ("signer_identity", manifest.signer_identity.as_str()),
    ] {
        validate_symbol(field, value)
            .map_err(|error| FamilyPluginLoadError::Manifest(error.to_string()))?;
    }
    let target_count = manifest
        .supported_targets
        .iter()
        .collect::<BTreeSet<_>>()
        .len();
    if manifest.schema != "astra.emu.native_plugin_manifest.v1"
        || manifest.engine_version.is_empty()
        || manifest.rustc_fingerprint.is_empty()
        || manifest.feature_fingerprint.is_empty()
        || manifest.abi_fingerprint.is_empty()
        || manifest.supported_targets.is_empty()
        || target_count != manifest.supported_targets.len()
        || manifest.engine_version != gate.engine_version
        || manifest.rustc_fingerprint != gate.rustc_fingerprint
        || manifest.feature_fingerprint != gate.feature_fingerprint
        || manifest.abi_fingerprint != gate.abi_fingerprint
        || !manifest.package_eligible
        || !manifest
            .supported_targets
            .iter()
            .any(|target| target == &gate.target)
        || !gate.allowed_signers.contains(&manifest.signer_identity)
        || manifest.signature_algorithm != "ed25519-v1"
        || manifest.signature_hex.len() != 128
        || (gate.require_native_manifest_binding && manifest.native_manifest_hash.is_none())
        || gate
            .expected_native_manifest_hash
            .is_some_and(|expected| manifest.native_manifest_hash != Some(expected))
    {
        return Err(FamilyPluginLoadError::Manifest(
            "plugin identity or eligibility gate failed".into(),
        ));
    }
    Ok(())
}

pub fn canonical_family_manifest_bytes(
    manifest: &FamilyPluginManifest,
) -> Result<Vec<u8>, FamilyPluginLoadError> {
    postcard::to_allocvec(&SignedFamilyPluginIdentity {
        schema: &manifest.schema,
        family_id: &manifest.family_id,
        plugin_id: &manifest.plugin_id,
        provider_id: &manifest.provider_id,
        engine_version: &manifest.engine_version,
        rustc_fingerprint: &manifest.rustc_fingerprint,
        feature_fingerprint: &manifest.feature_fingerprint,
        abi_fingerprint: &manifest.abi_fingerprint,
        binary_hash: manifest.binary_hash,
        signer_identity: &manifest.signer_identity,
        package_eligible: manifest.package_eligible,
        supported_targets: &manifest.supported_targets,
        native_manifest_hash: manifest.native_manifest_hash,
    })
    .map_err(|error| FamilyPluginLoadError::Manifest(error.to_string()))
}

/// Computes the invariant family identity used by Android's APK-bound native
/// manifest. The native-manifest hash is removed to avoid a circular hash:
/// the final signed family manifest binds the native manifest in the opposite
/// direction.
pub fn family_base_identity_hash(
    manifest: &FamilyPluginManifest,
) -> Result<Hash256, FamilyPluginLoadError> {
    let mut base = manifest.clone();
    base.native_manifest_hash = None;
    Ok(Hash256::from_sha256(&canonical_family_manifest_bytes(
        &base,
    )?))
}

pub fn inspect_dynamic_family_descriptor(
    path: impl AsRef<Path>,
) -> Result<LegacyFamilyPluginDescriptor, FamilyPluginLoadError> {
    let library = unsafe { Library::new(path.as_ref()) }
        .map_err(|_| FamilyPluginLoadError::AbiLoad("library"))?;
    let module = unsafe { root_module(&library)? };
    let descriptor: LegacyFamilyPluginDescriptor =
        native_result((module.descriptor())()).map_err(provider_error)?;
    descriptor.validate().map_err(provider_error)?;
    Ok(descriptor)
}

fn validate_descriptor_binding(
    manifest: &FamilyPluginManifest,
    descriptor: &LegacyFamilyPluginDescriptor,
) -> Result<(), FamilyPluginLoadError> {
    if descriptor.family_id.0 != manifest.family_id
        || descriptor.plugin_id != manifest.plugin_id
        || descriptor.provider_id != manifest.provider_id
        || descriptor.engine_version != manifest.engine_version
        || descriptor.rustc_fingerprint != manifest.rustc_fingerprint
        || descriptor.feature_fingerprint != manifest.feature_fingerprint
        || descriptor.abi_fingerprint != manifest.abi_fingerprint
    {
        return Err(FamilyPluginLoadError::Manifest(
            "loaded descriptor does not match native manifest".into(),
        ));
    }
    Ok(())
}

unsafe fn root_module(
    library: &Library,
) -> Result<AstraLegacyFamilyModuleRef, FamilyPluginLoadError> {
    let header = library
        .get::<AbiHeaderRef>(ROOT_MODULE_LOADER_NAME_WITH_NUL.as_bytes())
        .map_err(|_| FamilyPluginLoadError::AbiLoad("root_symbol"))?;
    let header = (*header)
        .upgrade()
        .map_err(|_| FamilyPluginLoadError::AbiLoad("abi_header"))?;
    header
        .init_root_module::<AstraLegacyFamilyModuleRef>()
        .map_err(|_| FamilyPluginLoadError::AbiLoad("root_init"))
}

fn host_services() -> &'static Mutex<BTreeMap<String, DynamicFamilyHostServicesV8>> {
    HOST_SERVICES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn get_host_services(token: &str) -> Result<DynamicFamilyHostServicesV8, LegacyProviderError> {
    host_services()
        .lock()
        .map_err(|_| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_HOST_SERVICE_LOCK_POISONED",
                "host service registry lock is poisoned",
            )
        })?
        .get(token)
        .cloned()
        .ok_or_else(|| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_HOST_TOKEN",
                "host service token is not active",
            )
        })
}

fn remove_host_services(token: &str) {
    if let Ok(mut services) = host_services().lock() {
        services.remove(token);
    }
}

fn provider_error(error: LegacyProviderError) -> FamilyPluginLoadError {
    FamilyPluginLoadError::Provider(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::process::Command;

    struct V8HostFixture;

    impl LegacyTextLayoutHostV8 for V8HostFixture {
        fn layout(
            &self,
            session: &LegacyRuntimeSessionId,
            request: LegacyTextLayoutRequestV8,
        ) -> Result<LegacyTextLayoutResultV8, LegacyProviderError> {
            Ok(LegacyTextLayoutResultV8 {
                layout_token: format!("{}.layout", session.0),
                width: request.width as f32,
                height: request.line_height,
                baseline: request.font_size,
                line_count: 1,
                glyph_count: request.text.chars().count() as u32,
                clipped: false,
                cache_revision: 1,
            })
        }

        fn release_layout(
            &self,
            _session: &LegacyRuntimeSessionId,
            _layout_token: &str,
        ) -> Result<(), LegacyProviderError> {
            Ok(())
        }
    }

    impl LegacyPrivateMaterialHostV8 for V8HostFixture {
        fn read_private_material(
            &self,
            _session: &LegacyRuntimeSessionId,
            request: LegacyPrivateMaterialRequestV8,
        ) -> Result<LegacySecretBufferV8, LegacyProviderError> {
            Ok(LegacySecretBufferV8::new(vec![
                7;
                request.exact_len as usize
            ]))
        }
    }

    impl LegacySaveStoreHostV8 for V8HostFixture {
        fn list_slots(
            &self,
            _session: &LegacyRuntimeSessionId,
            max_slots: u32,
        ) -> Result<Vec<LegacySaveSlotV8>, LegacyProviderError> {
            Ok((max_slots > 0)
                .then_some(LegacySaveSlotV8 {
                    slot_id: "slot-0".into(),
                    revision: 1,
                    byte_len: 3,
                })
                .into_iter()
                .collect())
        }

        fn read_slot(
            &self,
            _session: &LegacyRuntimeSessionId,
            slot_id: &str,
            _expected_revision: Option<u64>,
            _max_bytes: u64,
        ) -> Result<LegacySaveReadResultV8, LegacyProviderError> {
            Ok(LegacySaveReadResultV8 {
                slot_id: slot_id.into(),
                revision: 1,
                payload: vec![1, 2, 3].into(),
            })
        }

        fn atomic_write_slot(
            &self,
            _session: &LegacyRuntimeSessionId,
            request: LegacySaveWriteRequestV8,
        ) -> Result<LegacySaveWriteResultV8, LegacyProviderError> {
            Ok(LegacySaveWriteResultV8 {
                slot_id: request.slot_id,
                revision: 2,
                byte_len: request.payload.len() as u64,
                verified: true,
            })
        }
    }

    fn register_v8_host_fixture(token: &str) {
        let fixture = Arc::new(V8HostFixture);
        host_services().lock().unwrap().insert(
            token.into(),
            DynamicFamilyHostServicesV8 {
                vfs: Arc::new(DynamicMemoryVfs {
                    script: Vec::new(),
                    default_font: Vec::new(),
                }),
                text_layout: Some(fixture.clone()),
                private_material: Some(fixture.clone()),
                save_store: Some(fixture),
            },
        );
    }

    #[test]
    fn v8_instance_host_services_dispatch_with_bounds() {
        let token = "host-services.fixture";
        register_v8_host_fixture(token);
        let layout = native_result::<_, LegacyTextLayoutResultV8>(ffi_layout_text(
            token.into(),
            FfiTextLayoutCallV8 {
                session_id: "session-0".into(),
                request: LegacyTextLayoutRequestV8 {
                    lease_id: "lease-0".into(),
                    text: "abc".into(),
                    language: "en".into(),
                    font_families: vec!["Fixture".into()],
                    font_size: 16.0,
                    line_height: 20.0,
                    width: 320,
                    height: 200,
                    max_lines: 4,
                    wrap: LegacyTextWrapV8::Word,
                    overflow: LegacyTextOverflowV8::Clip,
                }
                .into(),
            },
        ))
        .unwrap();
        assert_eq!(layout.layout_token, "session-0.layout");

        let secret = native_result::<_, FfiSecretBufferV8>(ffi_read_private_material(
            token.into(),
            FfiPrivateMaterialCallV8 {
                session_id: "session-0".into(),
                secret_id: "siglus.scene-key".into(),
                exact_len: 16,
            },
        ))
        .unwrap();
        assert_eq!(secret.bytes.len(), 16);

        let write = native_result::<_, FfiSaveWriteResultV8>(ffi_atomic_write_save_slot(
            token.into(),
            FfiSaveWriteCallV8 {
                session_id: "session-0".into(),
                slot_id: "slot-0".into(),
                expected_revision: ROption::RSome(1),
                max_bytes: 3,
                payload: astra_byte_source::OwnedByteBuffer::from_vec(vec![1, 2, 3]).into_ffi(),
            },
        ))
        .unwrap();
        assert!(write.verified);
        remove_host_services(token);
    }

    fn manifest() -> FamilyPluginManifest {
        FamilyPluginManifest {
            schema: "astra.emu.native_plugin_manifest.v1".into(),
            family_id: "fvp".into(),
            plugin_id: "astra.emu.fvp".into(),
            provider_id: "astra.emu.fvp.runtime".into(),
            engine_version: "0.1.0".into(),
            rustc_fingerprint: "rustc-test".into(),
            feature_fingerprint: "features-test".into(),
            abi_fingerprint: LEGACY_FAMILY_ABI_FINGERPRINT.into(),
            binary_hash: Hash256::from_sha256(b"fixture"),
            signer_identity: "astra.official.test".into(),
            signature_algorithm: "ed25519-v1".into(),
            signature_hex: "00".repeat(64),
            package_eligible: true,
            supported_targets: vec!["x86_64-pc-windows-msvc".into()],
            native_manifest_hash: Some(Hash256::from_sha256(b"native-manifest")),
        }
    }

    fn gate() -> FamilyPluginGate {
        FamilyPluginGate {
            engine_version: "0.1.0".into(),
            rustc_fingerprint: "rustc-test".into(),
            feature_fingerprint: "features-test".into(),
            abi_fingerprint: LEGACY_FAMILY_ABI_FINGERPRINT.into(),
            target: "x86_64-pc-windows-msvc".into(),
            allowed_signers: ["astra.official.test".into()].into_iter().collect(),
            require_native_manifest_binding: true,
            expected_native_manifest_hash: Some(Hash256::from_sha256(b"native-manifest")),
        }
    }

    #[test]
    fn manifest_gate_rejects_every_identity_and_eligibility_mismatch() {
        assert!(validate_manifest(&manifest(), &gate()).is_ok());
        type ManifestMutation = Box<dyn Fn(&mut FamilyPluginManifest)>;
        let mutations: Vec<ManifestMutation> = vec![
            Box::new(|value| value.schema = "wrong".into()),
            Box::new(|value| value.engine_version = "wrong".into()),
            Box::new(|value| value.rustc_fingerprint = "wrong".into()),
            Box::new(|value| value.feature_fingerprint = "wrong".into()),
            Box::new(|value| value.abi_fingerprint = "wrong".into()),
            Box::new(|value| value.package_eligible = false),
            Box::new(|value| value.supported_targets.clear()),
            Box::new(|value| value.signer_identity = "untrusted".into()),
            Box::new(|value| value.native_manifest_hash = None),
            Box::new(|value| value.native_manifest_hash = Some(Hash256::from_sha256(b"different"))),
            Box::new(|value| {
                value
                    .supported_targets
                    .push(value.supported_targets[0].clone())
            }),
        ];
        for mutate in mutations {
            let mut candidate = manifest();
            mutate(&mut candidate);
            assert!(validate_manifest(&candidate, &gate()).is_err());
        }
    }

    #[test]
    fn manifest_gate_rejects_v5_without_compatibility_path() {
        let mut manifest = manifest();
        manifest.abi_fingerprint = "astra.emu.family_abi.v5".into();
        let error = validate_manifest(&manifest, &gate()).unwrap_err();
        assert!(matches!(error, FamilyPluginLoadError::Manifest(_)));
    }

    #[test]
    fn manifest_gate_rejects_v6_without_compatibility_path() {
        let mut manifest = manifest();
        manifest.abi_fingerprint = "astra.emu.family_abi.v6".into();
        let error = validate_manifest(&manifest, &gate()).unwrap_err();
        assert!(matches!(error, FamilyPluginLoadError::Manifest(_)));
    }

    #[test]
    fn manifest_gate_rejects_v7_without_compatibility_path() {
        let mut manifest = manifest();
        manifest.abi_fingerprint = "astra.emu.family_abi.v7".into();
        let error = validate_manifest(&manifest, &gate()).unwrap_err();
        assert!(matches!(error, FamilyPluginLoadError::Manifest(_)));
    }

    #[test]
    fn descriptor_binding_is_exact() {
        let manifest = manifest();
        let descriptor = LegacyFamilyPluginDescriptor {
            family_id: FamilyId(manifest.family_id.clone()),
            plugin_id: manifest.plugin_id.clone(),
            provider_id: manifest.provider_id.clone(),
            engine_version: manifest.engine_version.clone(),
            rustc_fingerprint: manifest.rustc_fingerprint.clone(),
            feature_fingerprint: manifest.feature_fingerprint.clone(),
            abi_fingerprint: manifest.abi_fingerprint.clone(),
            supported_formats: vec!["fvp-bin".into()],
            permissions: vec!["vfs.read".into()],
            report_redaction: "hash-only".into(),
            license: "MPL-2.0".into(),
        };
        assert!(validate_descriptor_binding(&manifest, &descriptor).is_ok());
        let mut mismatched = descriptor;
        mismatched.provider_id = "wrong".into();
        assert!(validate_descriptor_binding(&manifest, &mismatched).is_err());
    }

    #[test]
    fn detached_signature_binds_every_manifest_field_and_binary_hash() {
        let signing_key = SigningKey::from_bytes(&[7; 32]);
        let verifier = Ed25519FamilySignatureVerifier::new([(
            "astra.official.test".into(),
            signing_key.verifying_key().to_bytes(),
        )])
        .unwrap();
        let binary = b"fixture";
        let mut signed = manifest();
        signed.signature_hex = hex::encode(
            signing_key
                .sign(&canonical_family_manifest_bytes(&signed).unwrap())
                .to_bytes(),
        );
        verifier.verify_official_signature(binary, &signed).unwrap();
        PinnedStaticFamilyVerifier::new(
            Ed25519FamilySignatureVerifier::new([(
                "astra.official.test".into(),
                signing_key.verifying_key().to_bytes(),
            )])
            .unwrap(),
            Hash256::from_sha256(binary),
        )
        .verify_static_registration(&signed)
        .unwrap();
        let mut tampered = signed.clone();
        tampered.provider_id = "astra.emu.fvp.tampered".into();
        assert!(verifier
            .verify_official_signature(binary, &tampered)
            .is_err());
        assert!(verifier
            .verify_official_signature(b"different", &signed)
            .is_err());
    }

    #[test]
    fn signed_dynamic_fvp_package_runs_complete_provider_lifecycle() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(4)
            .expect("manager core must remain inside the workspace");
        let status = Command::new("cargo")
            .args([
                "build",
                "-p",
                "astra-emu-fvp",
                "--features",
                "dynamic-plugin-export",
            ])
            .current_dir(root)
            .status()
            .expect("cargo must be available to build the package-bound fixture");
        assert!(status.success());
        let binary_path = astra_plugin::dylib_path(root, "astra_emu_fvp");
        let binary = fs::read(&binary_path).expect("FVP cdylib must be built in this target root");
        let descriptor = inspect_dynamic_family_descriptor(&binary_path).unwrap();
        let signing_key = SigningKey::from_bytes(&[19; 32]);
        let signer = "astra.official.dynamic-test";
        let mut manifest = FamilyPluginManifest {
            schema: "astra.emu.native_plugin_manifest.v1".into(),
            family_id: descriptor.family_id.0.clone(),
            plugin_id: descriptor.plugin_id.clone(),
            provider_id: descriptor.provider_id.clone(),
            engine_version: descriptor.engine_version.clone(),
            rustc_fingerprint: descriptor.rustc_fingerprint.clone(),
            feature_fingerprint: descriptor.feature_fingerprint.clone(),
            abi_fingerprint: descriptor.abi_fingerprint.clone(),
            binary_hash: Hash256::from_sha256(&binary),
            signer_identity: signer.into(),
            signature_algorithm: "ed25519-v1".into(),
            signature_hex: "00".repeat(64),
            package_eligible: true,
            supported_targets: vec!["dynamic-test".into()],
            native_manifest_hash: None,
        };
        manifest.signature_hex = hex::encode(
            signing_key
                .sign(&canonical_family_manifest_bytes(&manifest).unwrap())
                .to_bytes(),
        );
        let verifier = Arc::new(
            Ed25519FamilySignatureVerifier::new([(
                signer.into(),
                signing_key.verifying_key().to_bytes(),
            )])
            .unwrap(),
        );
        let loader = DynamicFamilyLoader::new(
            FamilyPluginGate {
                engine_version: descriptor.engine_version,
                rustc_fingerprint: descriptor.rustc_fingerprint,
                feature_fingerprint: descriptor.feature_fingerprint,
                abi_fingerprint: descriptor.abi_fingerprint,
                target: "dynamic-test".into(),
                allowed_signers: [signer.into()].into_iter().collect(),
                require_native_manifest_binding: false,
                expected_native_manifest_hash: None,
            },
            verifier,
        );
        let script = terminal_hcb();
        let fingerprint = Hash256::from_sha256(&script);
        let mut provider = loader
            .load(
                &binary_path,
                manifest,
                "dynamic.test.instance".into(),
                DynamicFamilyHostServicesV8::vfs_only(Arc::new(DynamicMemoryVfs {
                    script,
                    default_font: include_bytes!(
                        "../../../../../Engine/Fixtures/PublicDomainFonts/NotoSansSC-Variable.ttf"
                    )
                    .to_vec(),
                })),
            )
            .unwrap();
        let ctx = dynamic_host_ctx();
        let probe = provider
            .probe(
                &ctx,
                LegacyProbeRequest {
                    root_mount_id: "mount.test".into(),
                    candidate_uris: vec!["script.hcb".into()],
                    marker_hashes: vec![fingerprint],
                    max_entries: 1,
                    max_metadata_bytes: 4096,
                },
            )
            .unwrap();
        assert_eq!(probe.confidence_permyriad, 10_000);
        let session = LegacyRuntimeSessionId("dynamic.test.session".into());
        provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: session.clone(),
                    case_fingerprint: fingerprint,
                    script_uri: "script.hcb".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 31,
                    compatibility_profile: "rfvp-v1".into(),
                    family_options: [
                        ("fvp.nls".into(), "utf8".into()),
                        ("fvp.pack_paths".into(), "[]".into()),
                    ]
                    .into_iter()
                    .collect(),
                },
            )
            .unwrap();
        let mut resource = provider
            .begin_session_resource_read(&ctx, &session, "default.ttf", 4 * 1024 * 1024)
            .unwrap();
        assert_eq!(
            resource.complete().unwrap_err().code(),
            "ASTRA_FVP_RESOURCE_READ"
        );
        let mut output = None;
        for tick_index in 1..=4 {
            let step = provider
                .step(
                    &ctx,
                    &session,
                    LegacyStepInput {
                        tick_index,
                        delta_ns: 16_666_667,
                        session_seed: 31,
                        mode: LegacyReplayMode::Live,
                        input_edges: Vec::new(),
                        await_results: Vec::new(),
                        provider_results: Vec::new(),
                        budget: LegacyStepBudget {
                            max_instructions: 32,
                            max_effects: 32,
                            max_trace_entries: 32,
                        },
                    },
                )
                .unwrap();
            let terminal = step.status == LegacyRuntimeStatus::Terminal;
            output = Some(step);
            if terminal {
                break;
            }
        }
        let output = output.expect("at least one step must run");
        assert!(matches!(
            output.status,
            LegacyRuntimeStatus::Active | LegacyRuntimeStatus::Terminal
        ));
        let snapshot = provider.save(&ctx, &session).unwrap();
        let restore = provider.restore(&ctx, &session, &snapshot).unwrap();
        assert!((1..=4).contains(&restore.restored_fixed_step));
        let shutdown = provider.shutdown(&ctx, &session).unwrap();
        assert_eq!(shutdown.final_state_revision, output.state_revision);
    }

    struct DynamicMemoryVfs {
        script: Vec<u8>,
        default_font: Vec<u8>,
    }

    impl LegacyVfsReader for DynamicMemoryVfs {
        fn stat_file(
            &self,
            mount_set_id: &str,
            uri: &str,
        ) -> Result<astra_byte_source::ByteSourceStat, LegacyProviderError> {
            if mount_set_id != "mount.test" {
                return Err(LegacyProviderError::invalid(
                    "TEST_VFS_NOT_FOUND",
                    "dynamic fixture is not present",
                ));
            }
            let bytes = match uri {
                "script.hcb" => &self.script,
                "default.ttf" => &self.default_font,
                _ => {
                    return Err(LegacyProviderError::invalid(
                        "TEST_VFS_NOT_FOUND",
                        "dynamic fixture is not present",
                    ));
                }
            };
            Ok(astra_byte_source::ByteSourceStat {
                len: bytes.len() as u64,
                revision: astra_byte_source::SourceRevision(1),
            })
        }

        fn read_file_range(
            &self,
            mount_set_id: &str,
            uri: &str,
            expected_revision: astra_byte_source::SourceRevision,
            range: astra_byte_source::ByteRange,
            max_bytes: u64,
        ) -> Result<astra_byte_source::RangeReadResult, LegacyProviderError> {
            let stat = self.stat_file(mount_set_id, uri)?;
            range.validate(stat.len, max_bytes).map_err(|error| {
                LegacyProviderError::invalid("TEST_VFS_BOUNDS", error.to_string())
            })?;
            if stat.revision != expected_revision {
                return Err(LegacyProviderError::invalid(
                    "TEST_VFS_REVISION",
                    "dynamic fixture revision changed",
                ));
            }
            let source = match uri {
                "script.hcb" => &self.script,
                "default.ttf" => &self.default_font,
                _ => unreachable!("stat_file already rejected the URI"),
            };
            let bytes = source[range.offset as usize..(range.offset + range.len) as usize].to_vec();
            Ok(astra_byte_source::RangeReadResult {
                range,
                revision: stat.revision,
                bytes: bytes.into(),
            })
        }

        fn enumerate_by_extension(
            &self,
            mount_set_id: &str,
            root: &str,
            extension_without_dot: &str,
            max_entries: u32,
        ) -> Result<Vec<LegacyVfsListedFile>, LegacyProviderError> {
            if mount_set_id != "mount.test" || !root.is_empty() || max_entries == 0 {
                return Err(LegacyProviderError::invalid(
                    "TEST_VFS_ENUMERATE",
                    "dynamic fixture enumeration is invalid",
                ));
            }
            match extension_without_dot {
                "hcb" => Ok(vec![LegacyVfsListedFile {
                    uri: "script.hcb".into(),
                    stat: self.stat_file(mount_set_id, "script.hcb")?,
                }]),
                "bin" => Ok(Vec::new()),
                _ => Err(LegacyProviderError::invalid(
                    "TEST_VFS_ENUMERATE",
                    "dynamic fixture extension is unsupported",
                )),
            }
        }
    }

    fn dynamic_host_ctx() -> LegacyRuntimeHostCtx {
        LegacyRuntimeHostCtx {
            case_id: "case.test".into(),
            package_id: "package.test".into(),
            package_hash: Hash256::from_sha256(b"package"),
            mount_set_id: "mount.test".into(),
            media_service_ids: vec!["astra.media".into()],
            permission_policy_id: "permission.test".into(),
            report_sink_id: "report.test".into(),
            target: "game".into(),
            profile: "test".into(),
        }
    }

    fn terminal_hcb() -> Vec<u8> {
        let mut bytes = 8u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0x04, 0, 0, 0]);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&[8, 0, 2, b'X', 0]);
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes
    }
}
