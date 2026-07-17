use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

use abi_stable::{
    library::{AbiHeaderRef, ROOT_MODULE_LOADER_NAME_WITH_NUL},
    std_types::{RString, RVec},
};
use astra_core::Hash256;
use astra_emu_family_api::*;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use libloading::Library;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

static VFS_READERS: OnceLock<Mutex<BTreeMap<String, Arc<dyn LegacyVfsReader>>>> = OnceLock::new();

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
    #[error("ASTRA_EMU_FAMILY_ABI_LOAD")]
    AbiLoad,
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
        vfs: Arc<dyn LegacyVfsReader>,
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
        let library =
            unsafe { Library::new(path.as_ref()) }.map_err(|_| FamilyPluginLoadError::AbiLoad)?;
        let module = unsafe { root_module(&library)? };
        let descriptor: LegacyFamilyPluginDescriptor = (module.descriptor())(RVec::new())
            .decode()
            .map_err(provider_error)?;
        descriptor.validate().map_err(provider_error)?;
        validate_descriptor_binding(&manifest, &descriptor)?;

        let host_token = format!("emu.vfs.{}", Hash256::from_sha256(instance_id.as_bytes()));
        let mut readers = vfs_readers()
            .lock()
            .map_err(|_| FamilyPluginLoadError::AbiLoad)?;
        if readers.insert(host_token.clone(), vfs).is_some() {
            return Err(FamilyPluginLoadError::Manifest(
                "host VFS token collision".into(),
            ));
        }
        drop(readers);
        let request = LegacyProviderInstanceRequest {
            instance_id: instance_id.clone(),
        };
        let payload = postcard::to_allocvec(&request)
            .map_err(|error| FamilyPluginLoadError::Provider(error.to_string()))?;
        let services = FfiLegacyHostServices {
            host_token: host_token.clone().into(),
            stat_vfs: ffi_stat_vfs,
            read_vfs_range: ffi_read_vfs_range,
        };
        if let Err(error) = (module.create_instance())(services, payload.into()).decode::<()>() {
            remove_vfs_reader(&host_token);
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
    _library: Library,
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
        invoke(
            self.module.probe(),
            &LegacyProbeCall {
                instance_id: self.instance_id.clone(),
                ctx: ctx.clone(),
                request,
            },
        )
    }

    fn open(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        request: LegacyOpenRequest,
    ) -> Result<LegacyRuntimeSessionId, LegacyProviderError> {
        let result: LegacyRuntimeSessionId = invoke(
            self.module.open(),
            &LegacyOpenCall {
                instance_id: self.instance_id.clone(),
                ctx: ctx.clone(),
                request,
            },
        )?;
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
        invoke(
            self.module.step(),
            &LegacyStepCall {
                instance_id: self.instance_id.clone(),
                ctx: ctx.clone(),
                session_id: session.clone(),
                input,
            },
        )
    }

    fn save(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
    ) -> Result<LegacySnapshotEnvelope, LegacyProviderError> {
        self.validate_session(ctx, session)?;
        invoke(
            self.module.save(),
            &LegacySessionCall {
                instance_id: self.instance_id.clone(),
                ctx: ctx.clone(),
                session_id: session.clone(),
            },
        )
    }

    fn restore(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
        snapshot: &LegacySnapshotEnvelope,
    ) -> Result<LegacyRestoreReport, LegacyProviderError> {
        self.validate_session(ctx, session)?;
        invoke(
            self.module.restore(),
            &LegacyRestoreCall {
                instance_id: self.instance_id.clone(),
                ctx: ctx.clone(),
                session_id: session.clone(),
                snapshot: snapshot.clone(),
            },
        )
    }

    fn shutdown(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
    ) -> Result<LegacyShutdownReport, LegacyProviderError> {
        self.validate_session(ctx, session)?;
        let report = invoke(
            self.module.shutdown(),
            &LegacySessionCall {
                instance_id: self.instance_id.clone(),
                ctx: ctx.clone(),
                session_id: session.clone(),
            },
        )?;
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
        let value: Option<FfiLegacyEphemeralText> = invoke(
            self.module.take_ephemeral_text(),
            &LegacyTextLeaseCall {
                instance_id: self.instance_id.clone(),
                ctx: ctx.clone(),
                session_id: session.clone(),
                lease_id: lease_id.to_owned(),
            },
        )?;
        Ok(value.map(Into::into))
    }

    fn read_session_resource(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
        resource_uri: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, LegacyProviderError> {
        self.validate_session(ctx, session)?;
        invoke(
            self.module.read_session_resource(),
            &LegacyResourceReadCall {
                instance_id: self.instance_id.clone(),
                ctx: ctx.clone(),
                session_id: session.clone(),
                resource_uri: resource_uri.to_owned(),
                max_bytes,
            },
        )
    }
}

impl DynamicLegacyRuntimeProvider {
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
            let _ = invoke::<_, LegacyShutdownReport>(
                self.module.shutdown(),
                &LegacySessionCall {
                    instance_id: self.instance_id.clone(),
                    ctx,
                    session_id: LegacyRuntimeSessionId(session_id),
                },
            );
        }
        if let Ok(payload) = postcard::to_allocvec(&LegacyProviderInstanceRequest {
            instance_id: self.instance_id.clone(),
        }) {
            let _ = (self.module.destroy_instance())(payload.into()).decode::<()>();
        }
        remove_vfs_reader(&self.host_token);
    }
}

fn invoke<I: Serialize, O: serde::de::DeserializeOwned>(
    function: FfiLegacyInvoke,
    input: &I,
) -> Result<O, LegacyProviderError> {
    let bytes = postcard::to_allocvec(input)
        .map_err(|error| LegacyProviderError::invalid("ASTRA_EMU_FFI_ENCODE", error.to_string()))?;
    function(bytes.into()).decode()
}

extern "C" fn ffi_stat_vfs(host_token: RString, payload: RVec<u8>) -> FfiLegacyResult {
    let result = (|| {
        let call: LegacyVfsStatCall = decode_ffi_request(payload)?;
        let readers = vfs_readers().lock().map_err(|_| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_LOCK_POISONED",
                "host VFS registry lock is poisoned",
            )
        })?;
        let reader = readers.get(host_token.as_str()).ok_or_else(|| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_HOST_TOKEN", "host VFS token is not active")
        })?;
        reader.stat_file(&call.mount_set_id, &call.uri)
    })();
    match result {
        Ok(stat) => FfiLegacyResult::success(&stat),
        Err(error) => FfiLegacyResult::failure(error),
    }
}

extern "C" fn ffi_read_vfs_range(host_token: RString, payload: RVec<u8>) -> FfiLegacyResult {
    let result = (|| {
        let call: LegacyVfsRangeCall = decode_ffi_request(payload)?;
        let readers = vfs_readers().lock().map_err(|_| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_LOCK_POISONED",
                "host VFS registry lock is poisoned",
            )
        })?;
        let reader = readers.get(host_token.as_str()).ok_or_else(|| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_HOST_TOKEN", "host VFS token is not active")
        })?;
        reader.read_file_range(
            &call.mount_set_id,
            &call.uri,
            call.expected_revision,
            call.range,
            call.max_bytes,
        )
    })();
    match result {
        Ok(range) => FfiLegacyResult::success(&range),
        Err(error) => FfiLegacyResult::failure(error),
    }
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
    let library =
        unsafe { Library::new(path.as_ref()) }.map_err(|_| FamilyPluginLoadError::AbiLoad)?;
    let module = unsafe { root_module(&library)? };
    let descriptor: LegacyFamilyPluginDescriptor = (module.descriptor())(RVec::new())
        .decode()
        .map_err(provider_error)?;
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
        .map_err(|_| FamilyPluginLoadError::AbiLoad)?;
    let header = (*header)
        .upgrade()
        .map_err(|_| FamilyPluginLoadError::AbiLoad)?;
    header
        .init_root_module::<AstraLegacyFamilyModuleRef>()
        .map_err(|_| FamilyPluginLoadError::AbiLoad)
}

fn vfs_readers() -> &'static Mutex<BTreeMap<String, Arc<dyn LegacyVfsReader>>> {
    VFS_READERS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn remove_vfs_reader(token: &str) {
    if let Ok(mut readers) = vfs_readers().lock() {
        readers.remove(token);
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
            .args(["build", "-p", "astra-emu-fvp"])
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
                Arc::new(DynamicMemoryVfs { script }),
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
                    family_options: [("fvp.nls".into(), "utf8".into())].into_iter().collect(),
                },
            )
            .unwrap();
        let output = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    tick_index: 1,
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
        assert_eq!(output.status, LegacyRuntimeStatus::Terminal);
        let snapshot = provider.save(&ctx, &session).unwrap();
        let restore = provider.restore(&ctx, &session, &snapshot).unwrap();
        assert_eq!(restore.restored_fixed_step, 1);
        let shutdown = provider.shutdown(&ctx, &session).unwrap();
        assert_eq!(shutdown.final_state_hash, output.state_hash);
    }

    struct DynamicMemoryVfs {
        script: Vec<u8>,
    }

    impl LegacyVfsReader for DynamicMemoryVfs {
        fn stat_file(
            &self,
            mount_set_id: &str,
            uri: &str,
        ) -> Result<astra_byte_source::ByteSourceStat, LegacyProviderError> {
            if mount_set_id != "mount.test" || uri != "script.hcb" {
                return Err(LegacyProviderError::invalid(
                    "TEST_VFS_NOT_FOUND",
                    "dynamic fixture is not present",
                ));
            }
            Ok(astra_byte_source::ByteSourceStat {
                len: self.script.len() as u64,
                revision: astra_byte_source::SourceRevision(Hash256::from_sha256(&self.script)),
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
            let bytes =
                self.script[range.offset as usize..(range.offset + range.len) as usize].to_vec();
            Ok(astra_byte_source::RangeReadResult {
                range,
                revision: stat.revision,
                content_hash: Hash256::from_sha256(&bytes),
                bytes,
            })
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
