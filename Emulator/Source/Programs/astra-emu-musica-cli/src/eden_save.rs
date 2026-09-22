use astra_emu_musica::eden_save::{EdenEdition, EdenSave, EdenSaveEncoding};
use clap::ValueEnum;
use std::{io::Read, path::Path};
#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum Edition {
    Japanese,
    English,
}
#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum Encoding {
    ShiftJis,
    Gbk,
    Windows1252,
}
pub(crate) fn export(
    game_dir: &Path,
    profile: &Path,
    slot: u32,
    output: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if output.extension().and_then(|value| value.to_str()) != Some("sav") {
        return Err("ASTRA_EMU_EDEN_SAVE_OUTPUT_NAME: output must end in .sav".into());
    }
    let thumbnail = output.with_extension("png");
    if std::fs::symlink_metadata(output).is_ok() || std::fs::symlink_metadata(&thumbnail).is_ok() {
        return Err("ASTRA_EMU_EDEN_SAVE_OUTPUT_EXISTS: choose a new output file".into());
    }
    let save = astra_emu_musica::eden_save::export_slot(game_dir, profile, slot)?;
    let bytes = save.save.encode()?;
    // Publish SAV last: native readers must never see a loadable slot without its preview.
    // If publication fails, any new PNG remains explicit and no existing file is replaced.
    super::private_output::write_new_private(&thumbnail, &save.thumbnail_png)?;
    super::private_output::write_new_private(output, &bytes)?;
    println!("Exported one native eden message checkpoint and its captured thumbnail into new files. Original-game readback was not performed.");
    Ok(())
}
pub(crate) fn inspect(
    file: &Path,
    edition: Edition,
    encoding: Encoding,
) -> Result<(), Box<dyn std::error::Error>> {
    let save = read(file, edition, encoding)?;
    let encoded = save.encode()?;
    if EdenSave::decode(&encoded, save.edition, save.encoding)? != save {
        return Err("save container roundtrip failed".into());
    }
    println!("Native container decoded and roundtripped: {} variables, {} backlog records. VM restoration was not performed.",save.variables.len(),save.backlog.len());
    Ok(())
}

pub(crate) fn check_restore(
    file: &Path,
    game_dir: &Path,
    profile: &Path,
    edition: Edition,
    encoding: Encoding,
) -> Result<(), Box<dyn std::error::Error>> {
    let save = read(file, edition, encoding)?;
    let archive = astra_emu_musica::mount_musica(game_dir, profile)?;
    let (vm, _) = astra_emu_musica::MusicaVm::from_eden_save(&save, &archive, 1)?;
    println!("Native VM checkpoint restored: {} backlog records. Device playback and native export were not performed.", vm.state().backlog.len());
    Ok(())
}

fn read(
    file: &Path,
    edition: Edition,
    encoding: Encoding,
) -> Result<EdenSave, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(file).map_err(|_| "save file could not be opened")?;
    let mut bytes = Vec::new();
    file.take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "save file could not be read")?;
    let edition = match edition {
        Edition::Japanese => EdenEdition::Japanese,
        Edition::English => EdenEdition::English,
    };
    let encoding = match encoding {
        Encoding::ShiftJis => EdenSaveEncoding::ShiftJis,
        Encoding::Gbk => EdenSaveEncoding::Gbk,
        Encoding::Windows1252 => EdenSaveEncoding::Windows1252,
    };
    Ok(EdenSave::decode(&bytes, edition, encoding)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_preserves_an_existing_companion_thumbnail() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("eden0020.sav");
        let thumbnail = output.with_extension("png");
        std::fs::write(&thumbnail, b"existing native thumbnail").unwrap();
        assert!(export(root.path(), Path::new("missing.profile"), 0, &output).is_err());
        assert!(!output.exists());
        assert_eq!(
            std::fs::read(thumbnail).unwrap(),
            b"existing native thumbnail"
        );
    }

    #[test]
    fn export_never_replaces_an_existing_target_even_if_the_source_is_invalid() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("eden0000.sav");
        std::fs::write(&output, b"original commercial slot").unwrap();
        assert!(export(root.path(), Path::new("missing.profile"), 0, &output).is_err());
        assert_eq!(std::fs::read(&output).unwrap(), b"original commercial slot");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
