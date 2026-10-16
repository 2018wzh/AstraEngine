use super::{NativeFileSystem, RfvpError, RfvpFile, RfvpFileKind, RfvpFileSystem, RfvpResult};
use rfvp::host_api::fs::RfvpStorage;
use std::sync::Arc;

struct NativeStorage {
    filesystem: NativeFileSystem,
    read_limit: usize,
}

impl NativeFileSystem {
    pub(super) fn shared_storage(&self, read_limit: usize) -> Arc<dyn RfvpStorage> {
        Arc::new(NativeStorage {
            filesystem: self.clone(),
            read_limit,
        })
    }
}

impl RfvpStorage for NativeStorage {
    fn read(&self, path: &str) -> RfvpResult<Vec<u8>> {
        self.filesystem
            .open_file(path)?
            .read_to_vec(self.read_limit)
    }

    fn read_prefix(&self, path: &str, max_len: usize) -> RfvpResult<Vec<u8>> {
        let mut file = self.filesystem.open_file(path)?;
        let count = usize::try_from(file.len()?.min(max_len.min(self.read_limit) as u64))
            .map_err(|_| RfvpError::CapacityExceeded)?;
        let mut bytes = vec![0; count];
        file.read_exact_at(0, &mut bytes)?;
        Ok(bytes)
    }

    fn write(&self, path: &str, bytes: &[u8]) -> RfvpResult<()> {
        self.filesystem.write_atomic(path, bytes)
    }

    fn exists(&self, path: &str) -> bool {
        self.filesystem
            .resolve_existing(path)
            .ok()
            .and_then(|path| NativeFileSystem::info(&path).ok())
            .is_some_and(|info| info.kind == RfvpFileKind::File)
    }

    fn remove(&self, path: &str) -> RfvpResult<()> {
        self.filesystem.clone().remove(path)
    }

    fn list(&self, directory: &str) -> RfvpResult<Vec<String>> {
        let mut names = Vec::new();
        self.filesystem.list_entries(directory, &mut |path, info| {
            if info.kind == RfvpFileKind::File {
                let name = path.rsplit('/').next().ok_or(RfvpError::InvalidData)?;
                names.push(name.to_owned());
            }
            Ok(())
        })?;
        Ok(names)
    }

    fn open(&self, path: &str) -> RfvpResult<Box<dyn RfvpFile + Send>> {
        Ok(Box::new(self.filesystem.open_file(path)?))
    }
}
