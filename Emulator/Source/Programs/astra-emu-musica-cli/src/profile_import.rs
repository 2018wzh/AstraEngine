//! Developer-only conversions into the production key.toml schema.
//! Source parsers never initialize a core, modify a game, or select another key.
use astra_emu_musica::MusicaKeyFile;
use std::{fs, io::ErrorKind, path::Path};

mod garbro;
mod garbro_nrbf;
mod json;
mod key;

pub use garbro::{import as import_garbro_profile, list_titles as list_garbro_titles};
pub use json::import_json_profile;
pub use key::import_key_profile;

fn ensure_new_output(output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if output.file_name().is_none() || !output.parent().is_some_and(Path::is_dir) {
        return Err(
            "ASTRA_EMU_PROFILE_OUTPUT_PARENT: select an existing private configuration directory"
                .into(),
        );
    }
    match fs::symlink_metadata(output) {
        Ok(_) => Err("ASTRA_EMU_PROFILE_OUTPUT_EXISTS".into()),
        Err(cause) if cause.kind() == ErrorKind::NotFound => Ok(()),
        Err(_) => Err(
            "ASTRA_EMU_PROFILE_OUTPUT_ACCESS: check access to the selected private destination"
                .into(),
        ),
    }
}

fn write_profile(output: &Path, key: &MusicaKeyFile) -> Result<(), Box<dyn std::error::Error>> {
    key.validate()?;
    let text = toml::to_string_pretty(key)
        .map_err(|_| "ASTRA_EMU_MUSICA_KEY_SERIALIZE: key.toml could not be encoded")?;
    if text.len() > 1024 * 1024 {
        return Err("ASTRA_EMU_PROFILE_BOUND: imported key.toml exceeds 1 MiB".into());
    }
    crate::private_output::write_new_private(output, text.as_bytes())
}

fn encode_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(text, "{byte:02x}").expect("writing to a String cannot fail");
    }
    text
}
