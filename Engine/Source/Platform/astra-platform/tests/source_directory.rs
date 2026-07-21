use astra_platform::{
    AuthorizedSourceDirectory, AuthorizedSourceDirectoryBackend, AuthorizedSourceFileStat,
    PlatformError, PlatformErrorCode,
};

struct Fixture;

impl AuthorizedSourceDirectoryBackend for Fixture {
    fn stat_relative(&self, _: &str) -> Result<AuthorizedSourceFileStat, PlatformError> {
        Ok(AuthorizedSourceFileStat { byte_length: 3 })
    }

    fn read_relative(&self, _: &str, _: u64) -> Result<Vec<u8>, PlatformError> {
        Ok(vec![1, 2, 3])
    }

    fn read_relative_range(
        &self,
        _: &str,
        offset: u64,
        length: u64,
        _: u64,
    ) -> Result<Vec<u8>, PlatformError> {
        let bytes = [1, 2, 3];
        Ok(bytes[offset as usize..(offset + length) as usize].to_vec())
    }
}

#[test]
fn opaque_source_directory_only_accepts_bounded_relative_paths() {
    let source = AuthorizedSourceDirectory::from_backend(Fixture);
    assert_eq!(
        source.stat_relative("DATA/MENU.dxr").unwrap().byte_length,
        3
    );
    assert_eq!(source.read_relative("READY.dxr", 3).unwrap(), vec![1, 2, 3]);
    assert_eq!(
        source.read_relative_range("READY.dxr", 1, 2, 2).unwrap(),
        vec![2, 3]
    );
    let error = source.read_relative("../READY.dxr", 3).unwrap_err();
    assert_eq!(error.code, PlatformErrorCode::PermissionDenied);
}
