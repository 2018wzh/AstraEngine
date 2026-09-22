use super::*;
use astra_core::Hash256;
use std::path::Path;

/// Native checkpoint and its preview captured by the same core save operation.
pub struct EdenSlotExport {
    pub save: EdenSave,
    pub thumbnail_png: Vec<u8>,
}

/// Read a core-owned slot and export its validated current VM checkpoint.
/// The caller owns output publication; this function never modifies any file.
pub fn export_slot(
    game_root: &Path,
    profile: &Path,
    slot: u32,
) -> Result<EdenSlotExport, CoreError> {
    let storage = crate::storage::Storage::new(game_root).map_err(storage_error)?;
    let saved = storage.read(slot).map_err(storage_error)?;
    if saved.logical_extent != [1024, 640] {
        return Err(checkpoint::unsupported());
    }
    let state =
        crate::MusicaVm::decode_native_save(&saved.vm).map_err(|_| checkpoint::unsupported())?;
    let archive = crate::mount_musica(game_root, profile)?;
    let identity = serde_json::to_vec(archive.manifest()).map_err(|_| checkpoint::unsupported())?;
    if Hash256::from_sha256(&identity) != saved.game {
        return Err(checkpoint::unsupported());
    }
    let loaded =
        crate::script_loader::load_script(&archive, &state.script_uri, state.script_encoding)
            .map_err(storage_error)?;
    if loaded.hash != state.script_hash || loaded.script.encoding != state.script_encoding {
        return Err(checkpoint::unsupported());
    }
    for sound in &saved.sounds {
        if sound.fade.is_some()
            || !sound.position.is_finite()
            || sound.position < 0.0
            || !sound.volume.is_finite()
            || !sound.pan.is_finite()
            || (sound.playing && !matches!(sound.id, 0 | 4))
        {
            return Err(checkpoint::unsupported());
        }
        if sound.playing {
            let logical = state
                .audio
                .get(&sound.id)
                .ok_or_else(checkpoint::unsupported)?;
            if sound.uri != logical.resource_uri
                || sound.repeat != logical.looped
                || sound.volume != f32::from(logical.volume_milli) / 1000.0
                || sound.pan != f32::from(logical.pan_milli) / 1000.0
            {
                return Err(checkpoint::unsupported());
            }
        }
    }
    if state.audio.get(&0).is_some_and(|audio| audio.playing)
        && !saved
            .sounds
            .iter()
            .any(|sound| sound.id == 0 && sound.playing)
    {
        return Err(checkpoint::unsupported());
    }
    let mut vm = crate::MusicaVm::new(
        state.script_uri,
        loaded.hash,
        loaded.script,
        state.session_seed,
    )
    .map_err(|_| checkpoint::unsupported())?;
    vm.restore_native_save(&saved.vm, 1)
        .map_err(|_| checkpoint::unsupported())?;
    let save = vm.export_eden_save()?;
    let frame = saved.card.texture().map_err(storage_error)?;
    let image = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba8.to_vec())
        .ok_or_else(checkpoint::unsupported)?;
    let thumbnail = image::imageops::resize(&image, 128, 80, image::imageops::FilterType::Triangle);
    let mut encoded = std::io::Cursor::new(Vec::new());
    thumbnail
        .write_to(&mut encoded, image::ImageFormat::Png)
        .map_err(|_| checkpoint::unsupported())?;
    Ok(EdenSlotExport {
        save,
        thumbnail_png: encoded.into_inner(),
    })
}

fn storage_error(cause: astra_emu_family_api::FamilyError) -> CoreError {
    CoreError::invalid(
        "ASTRA_EMU_EDEN_SAVE_SLOT",
        format!("core slot cannot be exported: {}", cause.code()),
    )
}
