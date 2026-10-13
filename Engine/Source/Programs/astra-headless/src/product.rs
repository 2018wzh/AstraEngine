use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use astra_headless_protocol::{ProductAdapterRegistry, ProductOpenRequest, ProductSession};
use astra_headless_vn_adapter::NativeVnProductAdapterFactory;
use astra_package::{
    AuthorizedSourceReader, ContainerCryptoProvider, ContainerError,
    SourceFingerprintCryptoProvider, SourceUnlockPolicy, SourceVerificationManifest,
};
use astra_platform::{HeadlessHostProfile, PlatformHostSession};

use super::*;

pub(crate) fn bootstrap_test_env(output: &Path, build_identity: &Path) -> Result<(), String> {
    let identity_hash = read_identity_hash(build_identity)?;
    fs::create_dir_all(output).map_err(|e| format!("ASTRA_HEADLESS_BOOTSTRAP_ROOT_FAILED: {e}"))?;
    let package =
        astra_package::PackageBuilder::build(astra_package::PackageBuildRequest::fixture(
            "astra.headless.empty",
            "headless-test",
            Vec::new(),
        ))
        .map_err(|e| format!("ASTRA_HEADLESS_EMPTY_PACKAGE_FAILED: {e}"))?;
    let package_path = output.join("empty.astrapkg");
    fs::write(&package_path, package.as_bytes())
        .map_err(|e| format!("ASTRA_HEADLESS_EMPTY_PACKAGE_WRITE_FAILED: {e}"))?;
    let package_hash = astra_core::Hash256::from_sha256(package.as_bytes()).to_string();
    let mut profile = HeadlessHostProfile::reference(
        "headless-test",
        "astra.headless.empty",
        identity_hash,
        package_hash,
    );
    profile.id = "worktree-local-test".into();
    let profile_path = output.join("headless-profile.json");
    write_atomic_json(&profile_path, &profile)?;
    fs::create_dir_all(output.join("artifacts"))
        .map_err(|e| format!("ASTRA_HEADLESS_ARTIFACT_ROOT_FAILED: {e}"))?;
    println!(
        "{}",
        serde_json::json!({"schema":"astra.headless_test_environment.v1","profile":"headless-profile.json","package":"empty.astrapkg","artifact_root":"artifacts","profile_hash":profile.hash().map_err(|e| e.to_string())?})
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_product_profile(
    package_path: &Path,
    target: &str,
    product_profile: &str,
    id: &str,
    namespace: &str,
    viewport_width: u32,
    viewport_height: u32,
    video_decode: &str,
    max_decode_output_bytes: u64,
    max_decoded_cache_bytes: u64,
    max_gpu_resource_bytes: u64,
    max_video_frames: u64,
    manifest_only: bool,
    output: &Path,
    build_identity: &Path,
) -> Result<(), String> {
    if target.trim().is_empty()
        || product_profile.trim().is_empty()
        || id.trim().is_empty()
        || namespace.trim().is_empty()
        || video_decode.trim().is_empty()
        || max_decode_output_bytes == 0
        || max_decoded_cache_bytes == 0
        || max_gpu_resource_bytes == 0
        || max_video_frames == 0
        || viewport_width == 0
        || viewport_height == 0
    {
        return Err("ASTRA_HEADLESS_PREPARE_PROFILE_ARGUMENT_INVALID".into());
    }
    let identity_hash = read_identity_hash(build_identity)?;
    let package_source: Arc<dyn astra_byte_source::BoundedByteSource> = Arc::new(
        astra_byte_source::FileByteSource::open(package_path).map_err(|error| {
            format!("ASTRA_HEADLESS_PREPARE_PROFILE_PACKAGE_READ_FAILED: {error}")
        })?,
    );
    let (reader, package_storage_hash) =
        astra_package::AstraContainerReader::open_storage_audited_source(package_source)
            .map_err(|error| format!("ASTRA_HEADLESS_PREPARE_PROFILE_PACKAGE_INVALID: {error}"))?;
    let manifest: astra_package::PackageManifest = reader
        .decode_postcard("package.manifest")
        .map_err(|error| format!("ASTRA_HEADLESS_PREPARE_PROFILE_MANIFEST_INVALID: {error}"))?;
    if manifest.profile != product_profile {
        return Err("ASTRA_HEADLESS_PREPARE_PROFILE_PRODUCT_PROFILE_MISMATCH".into());
    }
    let mut profile = HeadlessHostProfile::reference(
        target,
        manifest.package_id,
        identity_hash,
        // Platform package sources verify the complete stored container bytes. The
        // package content root is an internal section identity and can differ from
        // the storage hash when container metadata is present.
        package_storage_hash.to_string(),
    );
    profile.id = id.to_string();
    profile.product_profile = product_profile.to_string();
    profile.viewport_width = viewport_width;
    profile.viewport_height = viewport_height;
    profile.providers.video_decode = video_decode.to_string();
    profile.max_decode_output_bytes = max_decode_output_bytes;
    profile.max_decoded_cache_bytes = max_decoded_cache_bytes;
    profile.max_gpu_resource_bytes = max_gpu_resource_bytes;
    profile.max_video_frames = max_video_frames;
    profile.artifacts.namespace = namespace.to_string();
    if manifest_only {
        profile.artifacts.retention = astra_platform::HeadlessArtifactRetention::ManifestOnly;
    }
    astra_platform::validate_headless_host_profile(&profile).map_err(|error| error.to_string())?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("ASTRA_HEADLESS_PREPARE_PROFILE_ROOT_FAILED: {error}"))?;
    }
    write_atomic_json(output, &profile)
        .map_err(|error| format!("ASTRA_HEADLESS_PREPARE_PROFILE_WRITE_FAILED: {error}"))?;
    println!(
        "{}",
        serde_json::json!({
            "schema": "astra.headless_product_profile_preparation.v1",
            "profile_hash": profile.hash().map_err(|error| error.to_string())?,
            "package_hash": profile.package_hash,
            "build_fingerprint": profile.build_fingerprint,
            "target": profile.target,
            "product_profile": profile.product_profile,
        })
    );
    Ok(())
}

pub(crate) fn product_registry(
    package_crypto: Option<Arc<dyn ContainerCryptoProvider>>,
) -> Result<ProductAdapterRegistry, String> {
    let mut registry = ProductAdapterRegistry::default();
    let factory = match package_crypto {
        Some(crypto) => NativeVnProductAdapterFactory::with_package_crypto(crypto),
        None => NativeVnProductAdapterFactory::default(),
    };
    registry
        .register(Arc::new(factory))
        .map_err(|error| error.to_string())?;
    Ok(registry)
}

pub(crate) fn source_package_crypto(
    raw: &astra_package::AstraContainerReader,
    source_profile: Option<&Path>,
    source_root: Option<&Path>,
) -> Result<Option<Arc<dyn ContainerCryptoProvider>>, String> {
    let source_locked = raw.has_section("source.unlock");
    match (source_locked, source_profile, source_root) {
        (false, None, None) => Ok(None),
        (false, _, _) => Err(
            "ASTRA_HEADLESS_SOURCE_UNLOCK_UNEXPECTED: plaintext package must not receive source input"
                .into(),
        ),
        (true, Some(profile_path), Some(root)) => {
            let policy: SourceUnlockPolicy = raw
                .decode_postcard("source.unlock")
                .map_err(|error| format!("ASTRA_HEADLESS_SOURCE_POLICY_INVALID: {error}"))?;
            let profile_bytes = fs::read(profile_path)
                .map_err(|_| "ASTRA_HEADLESS_SOURCE_PROFILE_READ_FAILED".to_string())?;
            let profile: SourceVerificationManifest = serde_json::from_slice(&profile_bytes)
                .map_err(|_| "ASTRA_HEADLESS_SOURCE_PROFILE_INVALID".to_string())?;
            let mut reader = HeadlessSourceReader::open(root)?;
            let provider = SourceFingerprintCryptoProvider::unlock(&policy, &profile, &mut reader)
                .map_err(|error| format!("ASTRA_HEADLESS_SOURCE_UNLOCK_FAILED: {error}"))?;
            Ok(Some(Arc::new(provider)))
        }
        (true, _, _) => Err(
            "ASTRA_HEADLESS_SOURCE_UNLOCK_REQUIRED: source-locked package requires --source-profile and --source-root"
                .into(),
        ),
    }
}

pub(crate) struct HeadlessSourceReader {
    pub(crate) root: PathBuf,
}

impl HeadlessSourceReader {
    pub(crate) fn open(root: &Path) -> Result<Self, String> {
        let root = root
            .canonicalize()
            .map_err(|_| "ASTRA_HEADLESS_SOURCE_ROOT_INVALID".to_string())?;
        if !root.is_dir() {
            return Err("ASTRA_HEADLESS_SOURCE_ROOT_INVALID".into());
        }
        Ok(Self { root })
    }

    pub(crate) fn resolve(&self, relative_path: &str) -> Result<PathBuf, ContainerError> {
        astra_platform::validate_safe_relative_path(relative_path)
            .map_err(|_| ContainerError::Crypto("authorized source path is invalid".into()))?;
        let path = self
            .root
            .join(relative_path)
            .canonicalize()
            .map_err(|_| ContainerError::Crypto("authorized source file is missing".into()))?;
        if !path.starts_with(&self.root) {
            return Err(ContainerError::Crypto(
                "authorized source path escaped its root".into(),
            ));
        }
        Ok(path)
    }
}

impl AuthorizedSourceReader for HeadlessSourceReader {
    fn stat_relative(&mut self, relative_path: &str) -> Result<u64, ContainerError> {
        let path = self.resolve(relative_path)?;
        let metadata = path
            .metadata()
            .map_err(|_| ContainerError::Crypto("authorized source file is missing".into()))?;
        if !metadata.is_file() {
            return Err(ContainerError::Crypto(
                "authorized source entry is not a file".into(),
            ));
        }
        Ok(metadata.len())
    }

    fn read_relative_range(
        &mut self,
        relative_path: &str,
        offset: u64,
        length: u64,
        max_bytes: u64,
    ) -> Result<Vec<u8>, ContainerError> {
        let path = self.resolve(relative_path)?;
        let metadata = path
            .metadata()
            .map_err(|_| ContainerError::Crypto("authorized source file is missing".into()))?;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| ContainerError::Crypto("authorized source range overflowed".into()))?;
        if !metadata.is_file() || length == 0 || length > max_bytes || end > metadata.len() {
            return Err(ContainerError::Crypto(
                "authorized source range exceeds its read bound".into(),
            ));
        }
        let length: usize = length.try_into().map_err(|_| {
            ContainerError::Crypto("authorized source range exceeds addressable memory".into())
        })?;
        let mut file = std::fs::File::open(path)
            .map_err(|_| ContainerError::Crypto("authorized source file open failed".into()))?;
        use std::io::{Read, Seek, SeekFrom};
        file.seek(SeekFrom::Start(offset))
            .and_then(|_| {
                let mut bytes = vec![0_u8; length];
                file.read_exact(&mut bytes).map(|_| bytes)
            })
            .map_err(|_| ContainerError::Crypto("authorized source range read failed".into()))
    }
}

pub(crate) async fn open_product(
    registry: &ProductAdapterRegistry,
    profile: &HeadlessHostProfile,
    host: &PlatformHostSession,
    container: astra_package::AstraContainerReader,
    performance_observer: Option<Arc<dyn astra_headless_protocol::ProductPerformanceObserver>>,
) -> Result<Box<dyn ProductSession>, String> {
    registry
        .open(
            &profile.providers.product_adapter,
            ProductOpenRequest {
                package: astra_headless_protocol::ProductPackageSource::VerifiedContainer(
                    container,
                ),
                profile: profile.product_profile.clone(),
                target: profile.target.clone(),
                locale: None,
                width: profile.viewport_width,
                height: profile.viewport_height,
                max_video_frames: profile.max_video_frames,
                max_decode_output_bytes: profile.max_decode_output_bytes,
                max_decoded_cache_bytes: profile.max_decoded_cache_bytes,
                retain_audio_timeline: !matches!(
                    profile.artifacts.retention,
                    astra_platform::HeadlessArtifactRetention::ManifestOnly
                ),
                performance_observer,
                presentation_rate_hz: profile.presentation_rate_hz,
                platform: host.client.clone(),
            },
        )
        .await
        .map_err(|error| error.to_string())
}
