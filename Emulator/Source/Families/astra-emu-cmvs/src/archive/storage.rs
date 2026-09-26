use super::*;
use crate::{CmvsPs2aVmState, CmvsStringSegment};

fn storage_error() -> CoreError {
    invalid(
        "ASTRA_EMU_CMVS_STORAGE_PATH",
        "native storage directory is invalid or unavailable",
    )
}

impl CmvsArchive {
    /// Native save-root mode 0 uses the mounted game directory. Only create
    /// prefixes ending at a separator, as `sub_426BC0` does; never touch files.
    pub(crate) fn ensure_path_directories(
        &self,
        vm: &CmvsPs2aVmState,
        buffer_offset: u16,
    ) -> Result<(), CoreError> {
        let segments = vm
            .interpreter_string_buffers
            .get(&buffer_offset)
            .ok_or_else(storage_error)?;
        let [CmvsStringSegment::InterpreterPrefixField { field_offset: 7452 }, CmvsStringSegment::OwnedString(relative)] =
            segments.as_slice()
        else {
            return Err(storage_error());
        };
        native::validate_search_path(relative)?;
        let root = self.game_root.as_deref().ok_or_else(storage_error)?;
        let root = root.canonicalize().map_err(|_| storage_error())?;
        let relative = relative.replace('\\', "/");
        let Some((directories, _)) = relative.rsplit_once('/') else {
            return Ok(());
        };
        let mut current = root.clone();
        for component in directories.split('/') {
            current.push(component);
            match std::fs::symlink_metadata(&current) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    std::fs::create_dir(&current).map_err(|_| storage_error())?;
                }
                Err(_) => return Err(storage_error()),
            }
            current = current.canonicalize().map_err(|_| storage_error())?;
            if !current.starts_with(&root) || !current.is_dir() {
                return Err(storage_error());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(path: &str) -> CmvsPs2aVmState {
        let mut vm = CmvsPs2aVmState::new(0);
        vm.interpreter_string_buffers.insert(
            5404,
            vec![
                CmvsStringSegment::InterpreterPrefixField { field_offset: 7452 },
                CmvsStringSegment::OwnedString(path.into()),
            ],
        );
        vm
    }

    #[test]
    fn creates_only_separator_terminated_prefixes_and_preserves_files() {
        let (root, mut archive) = super::super::cache_tests::fixture();
        archive.bind_game_root(root.path().to_path_buf()).unwrap();
        archive
            .ensure_path_directories(&request("save\\nested\\"), 5404)
            .unwrap();
        let existing = root.path().join("save/nested/system.dat");
        std::fs::write(&existing, b"original save").unwrap();
        archive
            .ensure_path_directories(&request("save/nested/system.dat"), 5404)
            .unwrap();
        archive
            .ensure_path_directories(&request("save/absent"), 5404)
            .unwrap();
        assert!(!root.path().join("save/absent").exists());
        assert_eq!(std::fs::read(existing).unwrap(), b"original save");
        assert!(archive
            .ensure_path_directories(&request("save/nested/system.dat/"), 5404)
            .is_err());
        for path in ["../escape/", "save/../escape/", "/absolute/", "C:\\save\\"] {
            assert!(archive
                .ensure_path_directories(&request(path), 5404)
                .is_err());
        }
    }

    #[test]
    fn unknown_or_unresolved_prefix_is_rejected_before_filesystem_access() {
        let (_root, archive) = super::super::cache_tests::fixture();
        assert!(archive
            .ensure_path_directories(&request("save/"), 5404)
            .is_err());
        let mut vm = request("save/");
        vm.interpreter_string_buffers.get_mut(&5404).unwrap()[0] =
            CmvsStringSegment::InterpreterPrefixField { field_offset: 1 };
        assert!(archive.ensure_path_directories(&vm, 5404).is_err());
    }
}
