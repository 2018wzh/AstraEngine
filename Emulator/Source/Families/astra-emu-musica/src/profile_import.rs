//! Explicit, local-private profile imports shared by Manager and the CLI.
//! These APIs never initialize a core, modify a game, overwrite a profile, or
//! select another title/key when the requested source cannot be imported.
use crate::MusicaProfile;
use std::{fs, io::ErrorKind, path::Path};

mod garbro;
mod garbro_nrbf;
mod key;
pub mod private_output;

pub use garbro::{import as import_garbro_profile, list_titles as list_garbro_titles};
pub use key::import_key_profile;

fn ensure_new_output(output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if output.file_name().is_none() || !output.parent().is_some_and(Path::is_dir) {
        return Err("ASTRA_EMU_PROFILE_OUTPUT_PARENT: select an existing private configuration directory".into());
    }
    match fs::symlink_metadata(output) {
        Ok(_) => Err("ASTRA_EMU_PROFILE_OUTPUT_EXISTS".into()),
        Err(cause) if cause.kind() == ErrorKind::NotFound => Ok(()),
        Err(_) => Err("ASTRA_EMU_PROFILE_OUTPUT_ACCESS: check access to the selected private destination".into()),
    }
}

fn write_profile(output: &Path, profile: &MusicaProfile) -> Result<(), Box<dyn std::error::Error>> {
    profile.validate()?;
    let bytes = serde_json::to_vec_pretty(profile)?;
    if bytes.len() > 1024 * 1024 {
        return Err("ASTRA_EMU_PROFILE_BOUND: imported profile exceeds 1 MiB".into());
    }
    private_output::write_new_private(output, &bytes)
}
