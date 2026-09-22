use super::*;
use crate::eden_save::{EdenEdition, EdenSave, EdenSaveEncoding};
use crate::MusicaMountedVfs;
use std::io::Read;

pub(super) fn edition_field() -> ConfigField {
    ConfigField {
        id: "eden_import_edition".into(),
        label: "Native eden save edition".into(),
        group: "Files".into(),
        kind: ConfigKind::Enum {
            choices: vec!["english".into(), "japanese".into()].into(),
        },
        default: ConfigValue::Enum("english".into()),
    }
}

pub(super) fn read_import(
    root: &Path,
    name: &str,
    config: &[ConfigEntry],
    encoding: ScriptEncoding,
) -> FamilyResult<Option<EdenSave>> {
    if name.is_empty() {
        return Ok(None);
    }
    relative(name)?;
    let root = root
        .canonicalize()
        .map_err(|_| error("ASTRA_EMU_EDEN_SAVE_PATH", "game root is unavailable"))?;
    let path = root.join(name).canonicalize().map_err(|_| {
        error(
            "ASTRA_EMU_EDEN_SAVE_PATH",
            "native import file is unavailable",
        )
    })?;
    if !path.starts_with(&root) {
        return Err(error(
            "ASTRA_EMU_EDEN_SAVE_PATH",
            "native import file escapes the game directory",
        ));
    }
    let edition = match config
        .iter()
        .find(|entry| entry.id == "eden_import_edition")
        .map(|entry| &entry.value)
    {
        Some(ConfigValue::Enum(value)) if value == "english" => EdenEdition::English,
        Some(ConfigValue::Enum(value)) if value == "japanese" => EdenEdition::Japanese,
        _ => {
            return Err(error(
                "ASTRA_EMU_EDEN_SAVE_EDITION",
                "native import edition is invalid",
            ))
        }
    };
    let encoding = match encoding {
        ScriptEncoding::Gbk => EdenSaveEncoding::Gbk,
        ScriptEncoding::ShiftJis => EdenSaveEncoding::ShiftJis,
    };
    let file = std::fs::File::open(path).map_err(|_| {
        error(
            "ASTRA_EMU_EDEN_SAVE_READ",
            "native import file cannot be read",
        )
    })?;
    let mut bytes = Vec::new();
    file.take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            error(
                "ASTRA_EMU_EDEN_SAVE_READ",
                "native import file cannot be read",
            )
        })?;
    let save = EdenSave::decode(&bytes, edition, encoding).map_err(core_error)?;
    save.checkpoint().map_err(core_error)?;
    Ok(Some(save))
}

pub(super) fn restore_vm(
    vm: &mut MusicaVm,
    save: &EdenSave,
    archive: &MusicaMountedVfs,
    encoding: ScriptEncoding,
) -> FamilyResult<(String, Option<String>)> {
    let event = vm
        .restore_eden_save(
            save,
            |name| {
                crate::script_loader::load_script(archive, &format!("musica:/scr/{name}"), encoding)
                    .map(|loaded| loaded.script)
                    .map_err(|_| {
                        crate::CoreError::invalid(
                            "ASTRA_EMU_EDEN_SAVE_SCRIPT",
                            "native history script cannot be loaded",
                        )
                    })
            },
            1,
        )
        .map_err(core_error)?;
    match event {
        crate::MusicaVmEvent::Message { text, speaker, .. } => Ok((text, speaker)),
        _ => Err(error(
            "ASTRA_EMU_EDEN_SAVE_STATE",
            "native checkpoint did not restore a message",
        )),
    }
}

pub(super) fn restore_audio(audio: &Audio, state: &crate::MusicaRuntimeState) -> FamilyResult<()> {
    // The native slot stores resource/parameters, not a decoder playhead.
    // Restore those streams at their native load boundary.
    audio.restore(
        state
            .audio
            .iter()
            .map(|(id, sound)| crate::audio::SoundSnapshot {
                id: *id,
                uri: sound.resource_uri.clone(),
                position: 0.0,
                volume: f32::from(sound.volume_milli) / 1000.0,
                pan: f32::from(sound.pan_milli) / 1000.0,
                repeat: sound.looped,
                playing: sound.playing,
                fade: None,
            })
            .collect(),
    )
}
