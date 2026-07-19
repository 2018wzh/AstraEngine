use std::{fs, path::Path};

use configparser::ini::Ini;
use serde::{Deserialize, Serialize};

use crate::{
    error::{PatchError, PatchResult},
    filesystem::join_relative,
};

pub const SETTINGS: &[(&str, &str)] = &[
    ("FullScreen", "0"),
    ("UseTitleBar", "0"),
    ("CenterStage", "1"),
    ("ResizeStage", "0"),
    ("SwitchColorDepth", "0"),
];

const INI_PATH: &str = "SETUP.ini";
const PROJECTOR_INI_PATH: &str = "TsuiNoSoraProjector.ini";
const INI_CONTENT: &str = "[Settings]\r\nFullScreen=0\r\nUseTitleBar=0\r\nCenterStage=1\r\nResizeStage=0\r\nSwitchColorDepth=0\r\n";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowPolicyRecord {
    pub relative_path: String,
    pub launcher_relative_path: String,
    pub projector_relative_path: String,
    pub projector_ini_relative_path: String,
    pub enforcement: String,
    pub mode: String,
    pub stage_width: u32,
    pub stage_height: u32,
    pub settings: Vec<WindowSetting>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSetting {
    pub key: String,
    pub value: String,
}

pub fn record() -> WindowPolicyRecord {
    WindowPolicyRecord {
        relative_path: INI_PATH.to_owned(),
        launcher_relative_path: crate::WINDOW_LAUNCHER_NAME.to_owned(),
        projector_relative_path: crate::WINDOWED_PROJECTOR_NAME.to_owned(),
        projector_ini_relative_path: PROJECTOR_INI_PATH.to_owned(),
        enforcement: "director_ini_plus_verified_borderless_win32_frame".to_owned(),
        mode: "centered_fixed_borderless_window".to_owned(),
        stage_width: 800,
        stage_height: 600,
        settings: SETTINGS
            .iter()
            .map(|(key, value)| WindowSetting {
                key: (*key).to_owned(),
                value: (*value).to_owned(),
            })
            .collect(),
    }
}

pub fn write(root: &Path) -> PatchResult<()> {
    for relative_path in [INI_PATH, PROJECTOR_INI_PATH] {
        let path = join_relative(root, relative_path)?;
        fs::write(path, INI_CONTENT.as_bytes()).map_err(|error| {
            PatchError::io(
                "TSUI_PATCH_WINDOW_POLICY_WRITE_FAILED",
                "write Director projector window policy",
                error,
            )
        })?;
    }
    Ok(())
}

pub fn verify(root: &Path) -> PatchResult<()> {
    for relative_path in [INI_PATH, PROJECTOR_INI_PATH] {
        verify_file(root, relative_path)?;
    }
    Ok(())
}

fn verify_file(root: &Path, relative_path: &str) -> PatchResult<()> {
    let path = join_relative(root, relative_path)?;
    let bytes = fs::read(path).map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_WINDOW_POLICY_READ_FAILED",
            "read Director projector window policy",
            error,
        )
    })?;
    if !bytes.is_ascii() {
        return Err(PatchError::validation(
            "TSUI_PATCH_WINDOW_POLICY_ENCODING_INVALID",
            "SETUP.ini must be ASCII",
        ));
    }
    let text = String::from_utf8(bytes).map_err(|_| {
        PatchError::validation(
            "TSUI_PATCH_WINDOW_POLICY_ENCODING_INVALID",
            "SETUP.ini must be ASCII",
        )
    })?;
    let mut ini = Ini::new();
    ini.read(text).map_err(|_| {
        PatchError::validation(
            "TSUI_PATCH_WINDOW_POLICY_INVALID",
            "SETUP.ini is not a valid Director projector configuration",
        )
    })?;
    for (key, expected) in SETTINGS {
        if ini.get("Settings", key).as_deref() != Some(*expected) {
            return Err(PatchError::validation(
                "TSUI_PATCH_WINDOW_POLICY_MISMATCH",
                format!("SETUP.ini does not enforce {key}={expected}"),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn writes_and_reads_exact_window_policy() {
        let temp = tempdir().expect("tempdir");
        write(temp.path()).expect("write");
        verify(temp.path()).expect("verify");
        assert_eq!(
            fs::read(temp.path().join(INI_PATH)).expect("ini"),
            INI_CONTENT.as_bytes()
        );
        assert_eq!(
            fs::read(temp.path().join(PROJECTOR_INI_PATH)).expect("projector ini"),
            INI_CONTENT.as_bytes()
        );
    }
}
