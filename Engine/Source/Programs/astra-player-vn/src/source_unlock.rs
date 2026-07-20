use astra_package::{
    AuthorizedSourceReader, ContainerError, PackageReader, SourceFingerprintCryptoProvider,
    SourceUnlockPolicy, SourceVerificationManifest,
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

    fn read_relative(
        &mut self,
        relative_path: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, ContainerError> {
        self.source
            .read_relative(relative_path, max_bytes)
            .map_err(|error| {
                ContainerError::Crypto(format!(
                    "authorized source read failed: code={:?}",
                    error.code
                ))
            })
    }
}
