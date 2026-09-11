use super::{encode_global_savedata_v1, try_decode_global_savedata_v1, GlobalSaveDataV1};
use crate::host_api::{RfvpError, RfvpFile, RfvpFileSystem, RfvpResult};
use crate::subsystem::world::GameData;

const PATH: &str = "save/rfvp_global.bin";

pub(crate) fn load(game: &mut GameData, fs: &mut impl RfvpFileSystem) -> RfvpResult<bool> {
    let mut file = match fs.open(PATH) {
        Ok(file) => file,
        Err(RfvpError::NotFound) => return Ok(false),
        Err(error) => return Err(error),
    };
    let bytes =
        file.read_to_vec(super::MAX_GLOBAL_PAYLOAD_BYTES + super::GLOBAL_SAVE_FOOTER_LEN)?;
    let state = try_decode_global_savedata_v1(&bytes)
        .map_err(|_| RfvpError::InvalidData)?
        .ok_or(RfvpError::InvalidData)?;
    // Validate before applying any field: a corrupt or unrelated file must not
    // partially change the session or be replaced with default settings on exit.
    if state.version != 1
        || state.non_volatile_global_count != game.globals.non_volatile_count()
        || state.volatile_global_count != game.globals.volatile_count()
        || state.volatile_globals.len() != usize::from(state.volatile_global_count)
        || state.readed_text.len() != game.motion_manager.text_manager.readed_text.len()
    {
        return Err(RfvpError::InvalidData);
    }
    state.apply(game);
    tracing::info!(target: "rfvp::save", event = "rfvp.global_save.loaded");
    Ok(true)
}

pub(crate) fn save(game: &GameData, fs: &mut impl RfvpFileSystem) -> RfvpResult<()> {
    let bytes = encode_global_savedata_v1(&GlobalSaveDataV1::capture(game))
        .map_err(|_| RfvpError::InvalidData)?;
    fs.write_all(PATH, &bytes)?;
    tracing::info!(target: "rfvp::save", event = "rfvp.global_save.written");
    Ok(())
}

#[cfg(test)]
mod tests;
