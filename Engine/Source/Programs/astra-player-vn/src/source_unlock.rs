use astra_byte_source::BoundedByteSource;
use astra_core::Hash256;
use astra_package::{
    AstraContainerReader, AuthorizedSourceReader, ContainerError, PackageReader,
    SourceFingerprintCryptoProvider, SourceUnlockPolicy, SourceVerificationManifest,
};
use astra_platform::AuthorizedSourceDirectory;
use std::sync::Arc;

pub fn open_source_locked_package(
    package_bytes: &[u8],
    policy: &SourceUnlockPolicy,
    manifest: &SourceVerificationManifest,
    source: &AuthorizedSourceDirectory,
) -> Result<PackageReader, ContainerError> {
    let mut adapter = PlatformSourceReader { source };
    let crypto = SourceFingerprintCryptoProvider::unlock(policy, manifest, &mut adapter)?;
    PackageReader::open_source_locked(package_bytes, policy, "source.unlock", Arc::new(crypto))
}

pub fn open_source_locked_verified_container(
    container: AstraContainerReader,
    policy: &SourceUnlockPolicy,
    manifest: &SourceVerificationManifest,
    source: &AuthorizedSourceDirectory,
) -> Result<PackageReader, ContainerError> {
    let mut adapter = PlatformSourceReader { source };
    let crypto = SourceFingerprintCryptoProvider::unlock(policy, manifest, &mut adapter)?;
    PackageReader::open_source_locked_container(
        container,
        policy,
        "source.unlock",
        Arc::new(crypto),
    )
}

pub fn open_source_locked_package_source(
    package_source: Arc<dyn BoundedByteSource>,
    package_storage_hash: Hash256,
    policy: &SourceUnlockPolicy,
    manifest: &SourceVerificationManifest,
    source: &AuthorizedSourceDirectory,
) -> Result<PackageReader, ContainerError> {
    let mut adapter = PlatformSourceReader { source };
    let crypto = SourceFingerprintCryptoProvider::unlock(policy, manifest, &mut adapter)?;
    PackageReader::open_source_locked_source(
        package_source,
        package_storage_hash,
        policy,
        "source.unlock",
        Arc::new(crypto),
    )
}

struct PlatformSourceReader<'a> {
    source: &'a AuthorizedSourceDirectory,
}

impl AuthorizedSourceReader for PlatformSourceReader<'_> {
    fn stat_relative(&mut self, relative_path: &str) -> Result<u64, ContainerError> {
        self.source
            .stat_relative(relative_path)
            .map(|stat| stat.byte_length)
            .map_err(|error| {
                ContainerError::Crypto(format!(
                    "authorized source stat failed: code={:?}",
                    error.code
                ))
            })
    }

    fn read_relative_range(
        &mut self,
        relative_path: &str,
        offset: u64,
        length: u64,
        max_bytes: u64,
    ) -> Result<Vec<u8>, ContainerError> {
        self.source
            .read_relative_range(relative_path, offset, length, max_bytes)
            .map_err(|error| {
                ContainerError::Crypto(format!(
                    "authorized source range read failed: code={:?}",
                    error.code
                ))
            })
    }
}
