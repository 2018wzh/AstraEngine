use super::*;
use configparser::ini::Ini;
use std::path::Path;
mod registry;

const DEFAULT_FONT: &str = "ＭＳ ゴシック";
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

fn failure() -> CoreError {
    invalid(
        "ASTRA_EMU_CMVS_NATIVE_CONFIG",
        "native configuration is invalid or unavailable",
    )
}

impl CmvsArchive {
    pub(crate) fn initialize_native_caption(
        &self,
        vm: &mut crate::CmvsPs2aVmState,
    ) -> Result<(), CoreError> {
        let root = self.game_root.as_deref().ok_or_else(failure)?;
        vm.process_global_words.insert(
            crate::vm::NATIVE_FONT_SHIFT_JIS,
            u32::from(font_shift_jis(root)?),
        );
        Ok(())
    }

    /// 41D7F0 configures D08 slot zero before loading start.ps3. Script
    /// commands may subsequently replace that slot or add later search slots.
    pub(crate) fn initialize_native_script_search(
        &self,
        vm: &mut crate::CmvsPs2aVmState,
    ) -> Result<(), CoreError> {
        let root = self.game_root.as_deref().ok_or_else(failure)?;
        let path = string(
            &read_ini(root, "cmvs.cfg")?,
            "CMVS_SYSTEM_MAIN",
            "SCRIPT_INIT_PATH",
        )
        .unwrap_or_default();
        let (bytes, _, malformed) = encoding_rs::SHIFT_JIS.encode(&path);
        if malformed || bytes.len() >= 256 {
            return Err(failure());
        }
        native::validate_search_path(&path)?;
        vm.resource_search_paths
            .entry(crate::CmvsResourceSearchBank::Script)
            .or_default()
            .insert(0, path);
        Ok(())
    }

    pub(crate) fn native_default_font(&self) -> Result<String, CoreError> {
        let root = self.game_root.as_deref().ok_or_else(failure)?;
        resolve_font(root, &registry::SystemRegistry)
    }
}

fn font_shift_jis(root: &Path) -> Result<bool, CoreError> {
    Ok(integer(
        &read_ini(root, "initial.cfg")?,
        "CMVS_INIT_CFG",
        "FONT_SHIFT_JIS",
        1,
    )? != 0)
}

trait Registry {
    fn number(&self, key: &str, value: &str) -> Result<Option<u32>, CoreError>;
    fn string(&self, key: &str, value: &str) -> Result<Option<String>, CoreError>;
}

fn resolve_font(root: &Path, registry: &impl Registry) -> Result<String, CoreError> {
    let initial = read_ini(root, "initial.cfg")?;
    let key = string(&initial, "CMVS_INIT_CFG", "REG").unwrap_or_default();
    let requested = integer(&initial, "CMVS_INIT_CFG", "CONFIG_PUT", 0)?.unsigned_abs() % 4;
    let (mode, in_registry) = if requested == 3 {
        match registry.number(&key, "ConfigPut")? {
            Some(value) => (value & 3, value == 1),
            None => (0, false),
        }
    } else {
        (requested, requested == 1)
    };
    let save_root = integer(&initial, "CMVS_INIT_CFG", "SAVE_FOLDER", 0)? as u32;
    // Mode 2 reads cmvs.cfg from the selected native save root. External
    // Documents/AppData roots remain unbound, just like storage command 22.
    if mode == 2 && matches!(save_root, 1 | 2) {
        return Err(invalid(
            "ASTRA_EMU_CMVS_CONFIG_ROOT_UNBOUND",
            "external native configuration root is not bound",
        ));
    }
    let configured = if in_registry {
        let key = key.trim_end_matches('\\');
        registry.string(&format!("{key}\\param"), "MI_Font")?
    } else {
        None
    };
    let font = match configured {
        Some(font) => font,
        None => string(&read_ini(root, "cmvs.cfg")?, "MESSAGE_INFO", "FONT").unwrap_or_default(),
    };
    let font = if font.is_empty() {
        DEFAULT_FONT.to_owned()
    } else {
        font
    };
    let (bytes, _, invalid) = encoding_rs::SHIFT_JIS.encode(&font);
    if invalid || bytes.len() >= 64 || bytes.contains(&0) {
        return Err(failure());
    }
    Ok(font)
}

fn string(ini: &Ini, section: &str, key: &str) -> Option<String> {
    ini.get(section, key).map(|value| {
        if value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
        {
            value[1..value.len() - 1].to_owned()
        } else {
            value
        }
    })
}

fn integer(ini: &Ini, section: &str, key: &str, default: i32) -> Result<i32, CoreError> {
    string(ini, section, key).map_or(Ok(default), |value| value.parse().map_err(|_| failure()))
}

fn read_ini(root: &Path, filename: &str) -> Result<Ini, CoreError> {
    let mut ini = Ini::new();
    ini.set_comment_symbols(&[';']);
    ini.set_inline_comment_symbols(None);
    ini.set_multiline(false);
    ini.set_cascade_defaults(false);
    let root = root.canonicalize().map_err(|_| failure())?;
    let path = root.join(filename);
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(ini),
        Err(_) => return Err(failure()),
        Ok(_) => {}
    }
    let path = path.canonicalize().map_err(|_| failure())?;
    if !path.starts_with(&root) {
        return Err(failure());
    }
    let file = File::open(path).map_err(|_| failure())?;
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failure())?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(failure());
    }
    let text = if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        encoding_rs::UTF_16LE.decode_without_bom_handling_and_without_replacement(bytes)
    } else {
        encoding_rs::SHIFT_JIS.decode_without_bom_handling_and_without_replacement(&bytes)
    }
    .ok_or_else(failure)?;
    if text.contains('\0') {
        return Err(failure());
    }
    ini.read(text.into_owned()).map_err(|_| failure())?;
    Ok(ini)
}

#[cfg(test)]
mod tests;
