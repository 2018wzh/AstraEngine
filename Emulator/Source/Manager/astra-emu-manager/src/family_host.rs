use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use astra_emu_family_api::{
    LegacyFamilyHostServicesV9, LegacyHookHostV1, LegacyHookInvocationV1, LegacyHookResultV1,
    LegacyHookStatusV1, LegacyProviderError, LegacyRuntimeProvider, LegacyVfsReader,
    LEGACY_FAMILY_ABI_FINGERPRINT,
};
use astra_emu_family_support::{LegacySurfaceStoreV9, LocalPrivateWritableFileHostV1};
#[cfg(target_os = "android")]
use astra_emu_manager_core::{family_base_identity_hash, AndroidNativePluginManifest};
use astra_emu_manager_core::{
    DynamicFamilyLoader, Ed25519FamilySignatureVerifier, FamilyPluginGate, FamilyPluginManifest,
};
#[cfg(target_os = "ios")]
use astra_emu_manager_core::{
    PinnedStaticFamilyVerifier, StaticFamilyRegistration, StaticFamilyRegistry,
};

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_SURFACE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_SURFACE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_HOOK_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;

pub struct ManagerLoadedFamily {
    pub provider: Box<dyn LegacyRuntimeProvider>,
    pub surfaces: Arc<LegacySurfaceStoreV9>,
    pub hooks: Arc<ManagerHookRouter>,
}

#[derive(Default)]
pub struct ManagerHookRouter {
    bound: Mutex<Option<Arc<dyn LegacyHookHostV1>>>,
}

impl ManagerHookRouter {
    pub fn bind(&self, hook: Arc<dyn LegacyHookHostV1>) -> Result<(), String> {
        let mut bound = self
            .bound
            .lock()
            .map_err(|_| "ASTRA_EMU_HOOK_ROUTER_POISONED".to_owned())?;
        if bound.is_some() {
            return Err("ASTRA_EMU_HOOK_ROUTER_ALREADY_BOUND".into());
        }
        *bound = Some(hook);
        Ok(())
    }

    pub fn unbind(&self) -> Result<(), String> {
        *self
            .bound
            .lock()
            .map_err(|_| "ASTRA_EMU_HOOK_ROUTER_POISONED".to_owned())? = None;
        Ok(())
    }
}

impl LegacyHookHostV1 for ManagerHookRouter {
    fn invoke(
        &self,
        invocation: LegacyHookInvocationV1,
    ) -> Result<LegacyHookResultV1, LegacyProviderError> {
        if invocation.session_id.is_empty()
            || invocation.invocation_id.is_empty()
            || invocation.family_id.is_empty()
            || invocation.family_game_id.is_empty()
            || invocation.hook_id.is_empty()
            || invocation.timeout_ms == 0
            || invocation.payload.len() > MAX_HOOK_PAYLOAD_BYTES
        {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_HOOK_INVOCATION",
                "hook invocation identity, timeout, or payload bound is invalid",
            ));
        }
        match self
            .bound
            .lock()
            .map_err(|_| {
                LegacyProviderError::invalid(
                    "ASTRA_EMU_HOOK_ROUTER_POISONED",
                    "Hook router state is poisoned",
                )
            })?
            .clone()
        {
            Some(bound) => bound.invoke(invocation),
            None => Ok(LegacyHookResultV1 {
                status: LegacyHookStatusV1::Unbound,
                payload: Vec::new().into(),
                diagnostics: Vec::new(),
            }),
        }
    }
}

fn manager_host_services(
    vfs: Arc<dyn LegacyVfsReader>,
) -> Result<
    (
        LegacyFamilyHostServicesV9,
        Arc<LegacySurfaceStoreV9>,
        Arc<ManagerHookRouter>,
    ),
    String,
> {
    let surfaces = Arc::new(
        LegacySurfaceStoreV9::new(MAX_SURFACE_BYTES, MAX_TOTAL_SURFACE_BYTES)
            .map_err(|error| error.to_string())?,
    );
    let project = directories::ProjectDirs::from("dev", "AstraEngine", "AstraEMU")
        .ok_or_else(|| "ASTRA_EMU_WRITABLE_ROOT_UNAVAILABLE".to_owned())?;
    let writable_files = Arc::new(
        LocalPrivateWritableFileHostV1::new(
            project.data_local_dir().join("family-data").join("manager"),
        )
        .map_err(|error| error.to_string())?,
    );
    let hooks = Arc::new(ManagerHookRouter::default());
    Ok((
        LegacyFamilyHostServicesV9 {
            vfs,
            surfaces: surfaces.clone(),
            hooks: hooks.clone(),
            writable_files,
        },
        surfaces,
        hooks,
    ))
}

pub struct FamilyHostConfig {
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    manifest_path: PathBuf,
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    library_path: PathBuf,
}

impl FamilyHostConfig {
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    pub fn installed_for_executable(executable: &Path) -> Result<Self, String> {
        let install_root = executable.parent().ok_or("ASTRA_EMU_INSTALL_ROOT")?;
        let family_root = install_root.join("families").join("fvp");
        Ok(Self {
            manifest_path: family_root.join("manifest.json"),
            library_path: family_root.join(platform_library_name()),
        })
    }

    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    pub fn with_paths(manifest_path: PathBuf, library_path: PathBuf) -> Self {
        Self {
            manifest_path,
            library_path,
        }
    }

    pub fn from_process() -> Result<Self, String> {
        #[cfg(target_os = "ios")]
        {
            if env::args_os().len() != 1 {
                return Err("ASTRA_EMU_MANAGER_ARGUMENT_UNKNOWN".into());
            }
            return Ok(Self {});
        }
        #[cfg(target_os = "android")]
        {
            if env::args_os().len() != 1 {
                return Err("ASTRA_EMU_MANAGER_ARGUMENT_UNKNOWN".into());
            }
            return Ok(Self {});
        }
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            let executable = env::current_exe().map_err(|_| "ASTRA_EMU_EXECUTABLE_PATH")?;
            let installed = Self::installed_for_executable(&executable)?;
            let mut manifest_path = installed.manifest_path;
            let mut library_path = installed.library_path;
            let mut arguments = env::args_os().skip(1);
            while let Some(argument) = arguments.next() {
                if argument == "--family-manifest" {
                    manifest_path = PathBuf::from(
                        arguments
                            .next()
                            .ok_or("ASTRA_EMU_FAMILY_MANIFEST_ARGUMENT")?,
                    );
                } else if argument == "--family-library" {
                    library_path = PathBuf::from(
                        arguments
                            .next()
                            .ok_or("ASTRA_EMU_FAMILY_LIBRARY_ARGUMENT")?,
                    );
                } else {
                    return Err("ASTRA_EMU_MANAGER_ARGUMENT_UNKNOWN".into());
                }
            }
            Ok(Self {
                manifest_path,
                library_path,
            })
        }
    }

    pub fn create_provider(
        &self,
        vfs: Arc<dyn LegacyVfsReader>,
    ) -> Result<ManagerLoadedFamily, String> {
        let (host_services, surfaces, hooks) = manager_host_services(vfs)?;
        #[cfg(target_os = "ios")]
        {
            return create_static_ios_provider(host_services).map(|provider| ManagerLoadedFamily {
                provider,
                surfaces,
                hooks,
            });
        }
        #[cfg(target_os = "android")]
        {
            return create_dynamic_android_provider(host_services).map(|provider| {
                ManagerLoadedFamily {
                    provider,
                    surfaces,
                    hooks,
                }
            });
        }
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            let metadata =
                fs::metadata(&self.manifest_path).map_err(|_| "ASTRA_EMU_FAMILY_MANIFEST_READ")?;
            if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_MANIFEST_BYTES {
                return Err("ASTRA_EMU_FAMILY_MANIFEST_BOUNDS".into());
            }
            let manifest: FamilyPluginManifest = serde_json::from_slice(
                &fs::read(&self.manifest_path).map_err(|_| "ASTRA_EMU_FAMILY_MANIFEST_READ")?,
            )
            .map_err(|_| "ASTRA_EMU_FAMILY_MANIFEST_PARSE")?;
            let signer = env!("ASTRA_EMU_FAMILY_SIGNER_ID");
            let public_key = env!("ASTRA_EMU_FAMILY_PUBLIC_KEY_HEX");
            if signer.is_empty() || public_key.is_empty() {
                return Err("ASTRA_EMU_FAMILY_TRUST_ROOT_NOT_PROVISIONED".into());
            }
            let public_key =
                hex::decode(public_key).map_err(|_| "ASTRA_EMU_FAMILY_TRUST_ROOT_ENCODING")?;
            let public_key: [u8; 32] = public_key
                .try_into()
                .map_err(|_| "ASTRA_EMU_FAMILY_TRUST_ROOT_LENGTH")?;
            let verifier = Ed25519FamilySignatureVerifier::new([(signer.to_owned(), public_key)])
                .map_err(|error| error.to_string())?;
            let loader = DynamicFamilyLoader::new(
                FamilyPluginGate {
                    engine_version: env!("CARGO_PKG_VERSION").into(),
                    rustc_fingerprint: env!("ASTRA_EMU_MANAGER_RUSTC_FINGERPRINT").into(),
                    feature_fingerprint: env!("ASTRA_EMU_FVP_FEATURE_FINGERPRINT").into(),
                    abi_fingerprint: LEGACY_FAMILY_ABI_FINGERPRINT.into(),
                    target: env!("ASTRA_EMU_TARGET").into(),
                    allowed_signers: BTreeSet::from([signer.to_owned()]),
                    require_native_manifest_binding: cfg!(target_os = "android"),
                    expected_native_manifest_hash: None,
                },
                Arc::new(verifier),
            );
            let provider = loader
                .load(
                    &self.library_path,
                    manifest,
                    "astra.emu.manager.family.fvp".into(),
                    host_services,
                )
                .map_err(|error| error.to_string())?;
            Ok(ManagerLoadedFamily {
                provider: Box::new(provider),
                surfaces,
                hooks,
            })
        }
    }
}

#[cfg(target_os = "android")]
fn create_dynamic_android_provider(
    host_services: LegacyFamilyHostServicesV9,
) -> Result<Box<dyn LegacyRuntimeProvider>, String> {
    use astra_core::Hash256;

    let abi = android_abi()?;
    let family_bytes =
        crate::android_platform::read_asset(&format!("astraemu/families/fvp/{abi}/manifest.json"))?;
    let native_bytes = crate::android_platform::read_asset(&format!(
        "astraemu/families/fvp/{abi}/native-manifest.json"
    ))?;
    let family: FamilyPluginManifest =
        serde_json::from_slice(&family_bytes).map_err(|_| "ASTRA_EMU_FAMILY_MANIFEST_PARSE")?;
    let native: AndroidNativePluginManifest = serde_json::from_slice(&native_bytes)
        .map_err(|_| "ASTRA_EMU_ANDROID_NATIVE_MANIFEST_PARSE")?;
    let identity = crate::android_platform::package_identity()?;
    let expected_abi = abi;
    if native.schema != "astra.emu.android_native_plugin_manifest.v1"
        || native.min_api != 26
        || native.target_api != 36
        || identity.sdk_int < native.min_api
        || native.package_name != identity.package_name
        || native.version_code != identity.version_code
        || native.apk_signer_digest != identity.apk_signer_digest
        || native.abi != expected_abi
        || native.family_id != family.family_id
        || native.library_file_name != "libastra_emu_fvp.so"
        || native.library_hash != family.binary_hash
        || native.family_base_identity_hash
            != family_base_identity_hash(&family).map_err(|error| error.to_string())?
    {
        return Err("ASTRA_EMU_ANDROID_NATIVE_MANIFEST_IDENTITY".into());
    }
    let native_hash = native
        .identity_hash()
        .map_err(|_| "ASTRA_EMU_ANDROID_NATIVE_MANIFEST_HASH")?;
    if family.native_manifest_hash != Some(native_hash) {
        return Err("ASTRA_EMU_ANDROID_NATIVE_MANIFEST_BINDING".into());
    }
    let library_path = PathBuf::from(identity.native_library_dir).join(&native.library_file_name);
    let observed_library = fs::read(&library_path).map_err(|_| "ASTRA_EMU_FAMILY_LIBRARY_READ")?;
    if Hash256::from_sha256(&observed_library) != native.library_hash {
        return Err("ASTRA_EMU_ANDROID_NATIVE_LIBRARY_HASH".into());
    }
    let signer = env!("ASTRA_EMU_FAMILY_SIGNER_ID");
    let public_key = env!("ASTRA_EMU_FAMILY_PUBLIC_KEY_HEX");
    if signer.is_empty() || public_key.is_empty() {
        return Err("ASTRA_EMU_FAMILY_TRUST_ROOT_NOT_PROVISIONED".into());
    }
    let public_key = hex::decode(public_key).map_err(|_| "ASTRA_EMU_FAMILY_TRUST_ROOT_ENCODING")?;
    let public_key: [u8; 32] = public_key
        .try_into()
        .map_err(|_| "ASTRA_EMU_FAMILY_TRUST_ROOT_LENGTH")?;
    let verifier = Ed25519FamilySignatureVerifier::new([(signer.to_owned(), public_key)])
        .map_err(|error| error.to_string())?;
    let loader = DynamicFamilyLoader::new(
        FamilyPluginGate {
            engine_version: env!("CARGO_PKG_VERSION").into(),
            rustc_fingerprint: env!("ASTRA_EMU_MANAGER_RUSTC_FINGERPRINT").into(),
            feature_fingerprint: env!("ASTRA_EMU_FVP_FEATURE_FINGERPRINT").into(),
            abi_fingerprint: LEGACY_FAMILY_ABI_FINGERPRINT.into(),
            target: env!("ASTRA_EMU_TARGET").into(),
            allowed_signers: BTreeSet::from([signer.to_owned()]),
            require_native_manifest_binding: true,
            expected_native_manifest_hash: Some(native_hash),
        },
        Arc::new(verifier),
    );
    loader
        .load(
            &library_path,
            family,
            "astra.emu.manager.family.fvp".into(),
            host_services,
        )
        .map(|provider| Box::new(provider) as Box<dyn LegacyRuntimeProvider>)
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "android")]
fn android_abi() -> Result<&'static str, String> {
    match env!("ASTRA_EMU_TARGET") {
        "aarch64-linux-android" => Ok("arm64-v8a"),
        "x86_64-linux-android" => Ok("x86_64"),
        _ => Err("ASTRA_EMU_ANDROID_ABI_UNSUPPORTED".into()),
    }
}

#[cfg(target_os = "ios")]
fn create_static_ios_provider(
    host_services: LegacyFamilyHostServicesV9,
) -> Result<Box<dyn LegacyRuntimeProvider>, String> {
    use astra_core::Hash256;

    let manifest: FamilyPluginManifest = serde_json::from_slice(include_bytes!(concat!(
        env!("OUT_DIR"),
        "/astra-emu-fvp-static-manifest.json"
    )))
    .map_err(|_| "ASTRA_EMU_FAMILY_MANIFEST_PARSE")?;
    let signer = env!("ASTRA_EMU_FAMILY_SIGNER_ID");
    let public_key = hex::decode(env!("ASTRA_EMU_FAMILY_PUBLIC_KEY_HEX"))
        .map_err(|_| "ASTRA_EMU_FAMILY_TRUST_ROOT_ENCODING")?;
    let public_key: [u8; 32] = public_key
        .try_into()
        .map_err(|_| "ASTRA_EMU_FAMILY_TRUST_ROOT_LENGTH")?;
    let signature = Ed25519FamilySignatureVerifier::new([(signer.to_owned(), public_key)])
        .map_err(|error| error.to_string())?;
    let binary_hash: Hash256 = env!("ASTRA_EMU_FVP_STATIC_BINARY_HASH")
        .parse()
        .map_err(|_| "ASTRA_EMU_FAMILY_STATIC_HASH_INVALID")?;
    let verifier = PinnedStaticFamilyVerifier::new(signature, binary_hash);
    let mut registry = StaticFamilyRegistry::new(
        FamilyPluginGate {
            engine_version: env!("CARGO_PKG_VERSION").into(),
            rustc_fingerprint: env!("ASTRA_EMU_MANAGER_RUSTC_FINGERPRINT").into(),
            feature_fingerprint: env!("ASTRA_EMU_FVP_FEATURE_FINGERPRINT").into(),
            abi_fingerprint: LEGACY_FAMILY_ABI_FINGERPRINT.into(),
            target: env!("ASTRA_EMU_TARGET").into(),
            allowed_signers: BTreeSet::from([signer.to_owned()]),
            require_native_manifest_binding: false,
            expected_native_manifest_hash: None,
        },
        Arc::new(verifier),
    );
    registry
        .register(StaticFamilyRegistration {
            manifest,
            factory: astra_emu_fvp::create_static_fvp_provider,
        })
        .map_err(|error| error.to_string())?;
    registry
        .create("fvp", host_services)
        .map_err(|error| error.to_string())
}

fn platform_library_name() -> &'static Path {
    #[cfg(target_os = "windows")]
    {
        Path::new("astra_emu_fvp.dll")
    }
    #[cfg(target_os = "macos")]
    {
        Path::new("libastra_emu_fvp.dylib")
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        Path::new("libastra_emu_fvp.so")
    }
    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "android"
    )))]
    {
        Path::new("astra-emu-fvp.unsupported")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CompletingHook;

    impl LegacyHookHostV1 for CompletingHook {
        fn invoke(
            &self,
            invocation: LegacyHookInvocationV1,
        ) -> Result<LegacyHookResultV1, LegacyProviderError> {
            Ok(LegacyHookResultV1 {
                status: LegacyHookStatusV1::Completed,
                payload: invocation.payload,
                diagnostics: Vec::new(),
            })
        }
    }

    fn invocation() -> LegacyHookInvocationV1 {
        LegacyHookInvocationV1 {
            session_id: "session".into(),
            invocation_id: "invocation".into(),
            family_id: "minori".into(),
            family_game_id: "case".into(),
            hook_id: "astra.emu.translation.text.v1".into(),
            timeout_ms: 100,
            payload: b"text".to_vec().into(),
        }
    }

    #[test]
    fn hook_router_is_explicitly_bound_and_unbound() {
        let router = ManagerHookRouter::default();
        assert_eq!(
            router.invoke(invocation()).unwrap().status,
            LegacyHookStatusV1::Unbound
        );
        router.bind(Arc::new(CompletingHook)).unwrap();
        assert_eq!(
            router.invoke(invocation()).unwrap().status,
            LegacyHookStatusV1::Completed
        );
        assert_eq!(
            router.bind(Arc::new(CompletingHook)).unwrap_err(),
            "ASTRA_EMU_HOOK_ROUTER_ALREADY_BOUND"
        );
        router.unbind().unwrap();
        assert_eq!(
            router.invoke(invocation()).unwrap().status,
            LegacyHookStatusV1::Unbound
        );
    }

    #[test]
    fn hook_router_rejects_invalid_invocations_before_dispatch() {
        let router = ManagerHookRouter::default();
        let mut value = invocation();
        value.timeout_ms = 0;
        let error = router.invoke(value).unwrap_err();
        assert_eq!(error.code(), "ASTRA_EMU_HOOK_INVOCATION");
    }
}
