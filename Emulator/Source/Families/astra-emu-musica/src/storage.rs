use crate::{audio::SoundSnapshot, scene::error};
use astra_core::Hash256;
use astra_emu_family_api::FamilyResult;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};
mod card;
mod quick;
pub(crate) use card::SaveCard;

const MAGIC: &[u8; 8] = b"AMUSSV03";
const MAX_SAVE: usize = 16 * 1024 * 1024;
pub(crate) const SAVE_PAGE_WIDTH: u32 = 10;
pub(crate) const SAVE_PAGE_COUNT: u32 = 10;
pub(crate) const SAVE_MAX_SLOTS: u32 = SAVE_PAGE_WIDTH * SAVE_PAGE_COUNT;
pub(crate) const MANUAL_SAVE_FIRST_SLOT: u32 = 20;
#[derive(Serialize, Deserialize)]
pub(crate) struct Snapshot {
    pub card: SaveCard,
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
                    "ASTRA_EMU_MUSICA_SAVE_ROOT",
                    "game directory is unavailable",
                )
            })?,
        })
    }
    fn path(&self, slot: u32, create: bool) -> FamilyResult<PathBuf> {
        if slot >= SAVE_MAX_SLOTS {
            return Err(error(
                "ASTRA_EMU_MUSICA_SAVE_SLOT",
                "save slot is outside the native page range",
            ));
        }
        self.named_path(&format!("slot-{slot:03}.asav"), create)
    }
    fn named_path(&self, name: &str, create: bool) -> FamilyResult<PathBuf> {
        let directory = self.root.join(".astra-musica").join("saves");
        for path in [self.root.join(".astra-musica"), directory.clone()] {
            match fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                    return Err(error(
                        "ASTRA_EMU_MUSICA_SAVE_PATH",
                        "save directory is not a native directory",
                    ));
                }
                Ok(_) => {}
                Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => {
                    if create {
                        fs::create_dir(&path).map_err(|_| {
                            error(
                                "ASTRA_EMU_MUSICA_SAVE_DIRECTORY",
                                "save directory could not be created",
                            )
                        })?;
                    }
                }
                Err(_) => {
                    return Err(error(
                        "ASTRA_EMU_MUSICA_SAVE_PATH",
                        "save directory could not be inspected",
                    ))
                }
            }
        }
        let path = directory.join(name);
        match fs::symlink_metadata(&path) {
            Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
                return Err(error(
                    "ASTRA_EMU_MUSICA_SAVE_PATH",
                    "save slot is not a native regular file",
                ))
            }
            Ok(_) => {}
            Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err(error(
                    "ASTRA_EMU_MUSICA_SAVE_PATH",
                    "save slot could not be inspected",
                ))
            }
        }
        Ok(path)
    }
    pub fn read(&self, slot: u32) -> FamilyResult<Snapshot> {
        read(&self.path(slot, false)?)
    }
    pub fn card(&self, slot: u32, game: Hash256) -> FamilyResult<Option<SaveCard>> {
        let path = self.path(slot, false)?;
        if !path.try_exists().map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_SAVE_PATH",
                "save slot could not be inspected",
            )
        })? {
            return Ok(None);
        }
        let saved = read(&path)?;
        if saved.game != game {
            return Err(error(
                "ASTRA_EMU_MUSICA_SAVE_GAME",
                "save belongs to another game",
            ));
        }
        Ok(Some(saved.card))
    }
    pub fn write(&self, slot: u32, snapshot: &Snapshot) -> FamilyResult<()> {
        snapshot.card.validate()?;
        let path = self.path(slot, true)?;
        if path.try_exists().map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_SAVE_PATH",
                "save slot could not be inspected",
            )
        })? {
            let previous = read(&path)?;
            if previous.game != snapshot.game {
                return Err(error(
                    "ASTRA_EMU_MUSICA_SAVE_GAME",
                    "existing slot belongs to another game",
                ));
            }
        }
        let payload = postcard::to_allocvec(snapshot).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_SAVE_ENCODE",
                "save state cannot be encoded",
            )
        })?;
        if payload.len() > MAX_SAVE {
            return Err(error(
                "ASTRA_EMU_MUSICA_SAVE_BOUND",
                "save state exceeds the byte limit",
            ));
        }
        let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap()).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_SAVE_TEMP",
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
                    "ASTRA_EMU_MUSICA_SAVE_WRITE",
                    "save temporary file could not be flushed",
                )
            })?;
        temp.persist(&path).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_SAVE_REPLACE",
                "save atomic replacement failed",
            )
        })?;
        Ok(())
    }
}
fn read(path: &Path) -> FamilyResult<Snapshot> {
    let file = File::open(path).map_err(|_| {
        error(
            "ASTRA_EMU_MUSICA_SAVE_OPEN",
            "save slot could not be opened",
        )
    })?;
    let mut bytes = Vec::new();
    file.take((MAX_SAVE + 49) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| error("ASTRA_EMU_MUSICA_SAVE_READ", "save slot could not be read"))?;
    let fail = || {
        error(
            "ASTRA_EMU_MUSICA_SAVE_FORMAT",
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
    let saved: Snapshot = postcard::from_bytes(&bytes[48..]).map_err(|_| fail())?;
    saved.card.validate()?;
    Ok(saved)
}

#[cfg(test)]
#[path = "storage/tests.rs"]
mod tests;
