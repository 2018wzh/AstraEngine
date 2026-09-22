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
