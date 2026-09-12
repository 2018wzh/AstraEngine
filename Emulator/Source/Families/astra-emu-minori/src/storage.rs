use crate::{audio::SoundSnapshot, scene::error};
use astra_core::Hash256;
use astra_emu_family_api::FamilyResult;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};
const MAGIC: &[u8; 8] = b"AMINSV01";
const MAX_SAVE: usize = 16 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
pub(crate) struct Snapshot {
    pub game: Hash256,
    pub vm: Vec<u8>,
    pub message: Option<(String, Option<String>)>,
    pub wait_ns: u64,
    pub sounds: Vec<SoundSnapshot>,
}
pub(crate) struct Storage {
    root: PathBuf,
}
impl Storage {
    pub fn new(root: &Path) -> FamilyResult<Self> {
        Ok(Self {
            root: root.canonicalize().map_err(|_| {
                error(
                    "ASTRA_EMU_MINORI_SAVE_ROOT",
                    "game directory is unavailable",
                )
            })?,
        })
    }
    fn path(&self, create: bool) -> FamilyResult<PathBuf> {
        let directory = self.root.join(".astra-minori").join("saves");
        for path in [self.root.join(".astra-minori"), directory.clone()] {
            if let Ok(meta) = fs::symlink_metadata(&path) {
                if meta.file_type().is_symlink() || !meta.is_dir() {
                    return Err(error(
                        "ASTRA_EMU_MINORI_SAVE_PATH",
                        "save directory is not a native directory",
                    ));
                }
            } else if create {
                fs::create_dir(&path).map_err(|_| {
                    error(
                        "ASTRA_EMU_MINORI_SAVE_DIRECTORY",
                        "save directory could not be created",
                    )
                })?;
            }
        }
        let path = directory.join("slot-000.asav");
        if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
            return Err(error(
                "ASTRA_EMU_MINORI_SAVE_PATH",
                "save slot is not a native regular file",
            ));
        }
        Ok(path)
    }
    pub fn read(&self) -> FamilyResult<Snapshot> {
        read(&self.path(false)?)
    }
    pub fn write(&self, snapshot: &Snapshot) -> FamilyResult<()> {
        let path = self.path(true)?;
        if path.exists() {
            let _: Snapshot = read(&path)?;
        }
        let payload = postcard::to_allocvec(snapshot).map_err(|_| {
            error(
                "ASTRA_EMU_MINORI_SAVE_ENCODE",
                "save state cannot be encoded",
            )
        })?;
        if payload.len() > MAX_SAVE {
            return Err(error(
                "ASTRA_EMU_MINORI_SAVE_BOUND",
                "save state exceeds the byte limit",
            ));
        }
        let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap()).map_err(|_| {
            error(
                "ASTRA_EMU_MINORI_SAVE_TEMP",
                "save temporary file could not be created",
            )
        })?;
        temp.write_all(MAGIC)
            .and_then(|_| temp.write_all(&(payload.len() as u64).to_le_bytes()))
            .and_then(|_| temp.write_all(Hash256::from_sha256(&payload).as_bytes()))
            .and_then(|_| temp.write_all(&payload))
            .and_then(|_| temp.as_file().sync_all())
            .map_err(|_| {
                error(
                    "ASTRA_EMU_MINORI_SAVE_WRITE",
                    "save temporary file could not be flushed",
                )
            })?;
        temp.persist(&path).map_err(|_| {
            error(
                "ASTRA_EMU_MINORI_SAVE_REPLACE",
                "save atomic replacement failed",
            )
        })?;
        Ok(())
    }
}
fn read(path: &Path) -> FamilyResult<Snapshot> {
    let file = File::open(path).map_err(|_| {
        error(
            "ASTRA_EMU_MINORI_SAVE_OPEN",
            "save slot could not be opened",
        )
    })?;
    let mut bytes = Vec::new();
    file.take((MAX_SAVE + 49) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| error("ASTRA_EMU_MINORI_SAVE_READ", "save slot could not be read"))?;
    let fail = || {
        error(
            "ASTRA_EMU_MINORI_SAVE_FORMAT",
            "save slot is corrupt or belongs to an unsupported format",
        )
    };
    if bytes.len() < 48 || bytes.len() > MAX_SAVE + 48 || &bytes[..8] != MAGIC {
        return Err(fail());
    }
    let size = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
    if size != bytes.len() as u64 - 48
        || Hash256::from_sha256(&bytes[48..]).as_bytes() != &bytes[16..48]
    {
        return Err(fail());
    }
    postcard::from_bytes(&bytes[48..]).map_err(|_| fail())
}
