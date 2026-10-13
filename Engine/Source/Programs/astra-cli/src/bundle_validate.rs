use std::{fs, path::Path};

use astra_core::Hash256;
use astra_package::{PackageReader, ScenarioReference, ScenarioRefsManifest};
use astra_platform::PlatformId;
use astra_target::{TargetKind, TargetManifest};
use astra_vn_package::{load_player_locale_config, PlayerLocaleConfig};

use super::*;

pub(crate) fn validate_android_native_library(
    path: &Path,
    bytes: &[u8],
    abi: &str,
) -> Result<(), CliError> {
    if path.file_name().and_then(std::ffi::OsStr::to_str) != Some("libastra_player_android.so")
        || bytes.len() < 64
        || !bytes.starts_with(b"\x7fELF")
    {
        return Err(format!("ASTRA_ANDROID_NATIVE_LIBRARY_INVALID: {abi}").into());
    }
    let expected_machine = match abi {
        "arm64-v8a" => 183u16,
        "x86_64" => 62u16,
        _ => return Err("ASTRA_ANDROID_ABI_UNSUPPORTED".into()),
    };
    let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    if bytes[4] != 2 || bytes[5] != 1 || machine != expected_machine {
        return Err(format!("ASTRA_ANDROID_NATIVE_LIBRARY_ABI_MISMATCH: {abi}").into());
    }
    Ok(())
}

pub(crate) fn validate_web_player_wasm(bytes: &[u8]) -> Result<(), CliError> {
    wasmparser::Validator::new()
        .validate_all(bytes)
        .map_err(|error| format!("ASTRA_WEB_PLAYER_WASM_INVALID: {error}"))?;
    Ok(())
}

pub(crate) fn validate_web_player_glue(bytes: &[u8]) -> Result<(), CliError> {
    let source = std::str::from_utf8(bytes)
        .map_err(|error| format!("ASTRA_WEB_PLAYER_GLUE_INVALID: non-UTF-8 module: {error}"))?;
    for required in ["astra_player_web_bg.wasm", "WebAssembly"] {
        if !source.contains(required) {
            return Err(format!(
                "ASTRA_WEB_PLAYER_GLUE_INVALID: missing required wasm-bindgen marker {required}"
            )
            .into());
        }
    }
    if !source.contains("export default") && !source.contains(" as default") {
        return Err(
            "ASTRA_WEB_PLAYER_GLUE_INVALID: missing wasm-bindgen default initializer export".into(),
        );
    }
    for forbidden in [
        "astra-route-report",
        "AstraPlayer.route_model",
        "--dump-dom",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "ASTRA_WEB_PLAYER_GLUE_BYPASS: forbidden route/input bypass marker {forbidden}"
            )
            .into());
        }
    }
    Ok(())
}

pub(crate) struct PlayerConfigInput<'a> {
    pub(crate) target: &'a str,
    pub(crate) profile: &'a str,
    pub(crate) platform_name: &'a str,
    pub(crate) package_storage_hash: &'a str,
    pub(crate) display_config: &'a Option<PlayerDisplayConfig>,
    pub(crate) locale_config: &'a PlayerLocaleConfig,
    pub(crate) source_unlock_profile: Option<&'a str>,
}

pub(crate) fn player_config_bytes(input: PlayerConfigInput<'_>) -> Result<Vec<u8>, CliError> {
    let observability = if input.platform_name == "windows" {
        serde_json::json!({
            "filter": "warn",
            "console_format": "compact",
            "log_dir": "Saved/Logs",
            "crash_dir": "Saved/Crashes",
            "crash_reporting": "required"
        })
    } else {
        serde_json::json!({
            "filter": "info",
            "console_format": "json",
            "crash_reporting": "disabled"
        })
    };
    let mut config = serde_json::json!({
        "schema": "astra.player_config.v2",
        "target": input.target,
        "profile": input.profile,
        "platform": input.platform_name,
        "locale": input.locale_config.default_locale,
        "package": "package/nativevn.astrapkg",
        "package_storage_hash": input.package_storage_hash,
        "observability": observability
    });
    if let Some(display) = input.display_config {
        config["display"] = serde_json::to_value(display)?;
    }
    if let Some(source_profile) = input.source_unlock_profile {
        config["source_unlock"] = serde_json::json!({
            "schema": "astra.player_source_unlock.v1",
            "source_profile": source_profile
        });
    }
    serde_json::to_vec_pretty(&config).map_err(Into::into)
}

pub(crate) fn package_player_display_config(
    reader: &PackageReader,
) -> Result<Option<PlayerDisplayConfig>, CliError> {
    if !reader.has_section("player.display_config") {
        return Ok(None);
    }
    let display: PlayerDisplayConfig =
        serde_json::from_slice(&reader.container().read_section("player.display_config")?)?;
    validate_player_display_config(&display)?;
    Ok(Some(display))
}

pub(crate) fn package_player_locale_config(
    reader: &PackageReader,
) -> Result<PlayerLocaleConfig, CliError> {
    load_player_locale_config(reader).map_err(|error| error.to_string().into())
}

pub(crate) fn validate_player_display_config(
    display: &PlayerDisplayConfig,
) -> Result<(), CliError> {
    if display.schema != "astra.player_display_config.v1" {
        return Err("unsupported player display config schema".into());
    }
    if !(1..=16_384).contains(&display.original_resolution.width)
        || !(1..=16_384).contains(&display.original_resolution.height)
    {
        return Err("player display original resolution is out of range".into());
    }
    if !matches!(display.scale_filter.as_str(), "nearest" | "linear") {
        return Err("player display scale_filter must be nearest or linear".into());
    }
    for layer in &display.preview_layers {
        validate_player_display_layer_uri(&layer.vfs_uri)?;
        if layer.x > 16_384 || layer.y > 16_384 {
            return Err("player display preview layer coordinates are out of range".into());
        }
    }
    Ok(())
}

pub(crate) fn bundle_mount_policy(
    reader: &PackageReader,
    out: &Path,
    files: &mut Vec<StandaloneBundleFile>,
) -> Result<Option<String>, CliError> {
    if !reader.has_section("tsuinosora.mount_policy") {
        return Ok(None);
    }
    let bytes = reader
        .container()
        .read_bounded("tsuinosora.mount_policy", 256 * 1024)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    if json_has_local_path_like(&value) {
        return Err("tsuinosora mount policy contains path-like values".into());
    }
    let path = "AstraPlayer.mount_policy.json";
    fs::write(out.join(path), &bytes)?;
    files.push(bundle_file(path, "mount_policy", &bytes));
    Ok(Some(path.to_string()))
}

pub(crate) fn json_has_local_path_like(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(text) => looks_like_local_path(text),
        serde_json::Value::Array(items) => items.iter().any(json_has_local_path_like),
        serde_json::Value::Object(entries) => entries.values().any(json_has_local_path_like),
        _ => false,
    }
}

pub(crate) fn looks_like_local_path(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with("\\\\")
        || value
            .as_bytes()
            .windows(2)
            .any(|pair| pair[0].is_ascii_alphabetic() && pair[1] == b':')
        || value.split(['/', '\\']).any(|part| part == "..")
}

pub(crate) fn package_scenario_refs(
    reader: &PackageReader,
) -> Result<Vec<ScenarioReference>, CliError> {
    let bytes = reader.container().read_section("scenario.refs")?;
    let mut manifest: ScenarioRefsManifest = serde_json::from_slice(&bytes)?;
    manifest
        .scenarios
        .sort_by(|left, right| left.path.cmp(&right.path));
    Ok(manifest.scenarios)
}

pub(crate) fn bundle_file(path: &str, role: &str, bytes: &[u8]) -> StandaloneBundleFile {
    StandaloneBundleFile {
        path: path.to_string(),
        role: role.to_string(),
        hash: Hash256::from_sha256(bytes).to_string(),
        byte_size: bytes.len() as u64,
    }
}

pub(crate) fn make_executable(path: &std::path::Path) -> Result<(), CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

pub(crate) fn validate_universal_macho(bytes: &[u8]) -> Result<(), CliError> {
    const FAT_MAGIC: u32 = 0xcafebabe;
    const CPU_X86_64: u32 = 0x01000007;
    const CPU_ARM64: u32 = 0x0100000c;
    if bytes.len() < 8 || u32::from_be_bytes(bytes[0..4].try_into()?) != FAT_MAGIC {
        return Err("ASTRA_MACOS_UNIVERSAL_REQUIRED: player is not a fat Mach-O".into());
    }
    let count = usize::try_from(u32::from_be_bytes(bytes[4..8].try_into()?))?;
    let table_end = 8usize
        .checked_add(
            count
                .checked_mul(20)
                .ok_or("Mach-O architecture table overflow")?,
        )
        .filter(|end| *end <= bytes.len())
        .ok_or("Mach-O architecture table is truncated")?;
    let mut architectures = std::collections::BTreeSet::new();
    for record in bytes[8..table_end].as_chunks::<20>().0.iter() {
        architectures.insert(u32::from_be_bytes(record[0..4].try_into()?));
    }
    if !architectures.contains(&CPU_X86_64) || !architectures.contains(&CPU_ARM64) {
        return Err("ASTRA_MACOS_UNIVERSAL_REQUIRED: x86_64 and arm64 slices are required".into());
    }
    Ok(())
}

pub(crate) fn macos_info_plist(bundle_id: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n<key>CFBundleDevelopmentRegion</key><string>en</string>\n<key>CFBundleExecutable</key><string>astra-player</string>\n<key>CFBundleIdentifier</key><string>{bundle_id}</string>\n<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>\n<key>CFBundleName</key><string>AstraPlayer</string>\n<key>CFBundlePackageType</key><string>APPL</string>\n<key>LSMinimumSystemVersion</key><string>13.0</string>\n<key>NSHighResolutionCapable</key><true/>\n</dict></plist>\n"
    )
}

pub(crate) fn platform_id_name(platform: PlatformId) -> &'static str {
    match platform {
        PlatformId::Windows => "windows",
        PlatformId::Linux => "linux",
        PlatformId::Macos => "macos",
        PlatformId::Ios => "ios",
        PlatformId::Android => "android",
        PlatformId::Web => "web",
    }
}

pub(crate) fn package_target_manifest(
    manifest: &TargetManifest,
    target: &str,
) -> Result<TargetManifest, CliError> {
    let target = manifest
        .find(target)
        .ok_or_else(|| format!("target {target} is not defined"))?;
    if target.kind != TargetKind::Game || !target.packaged {
        return Err(format!(
            "package target {} must be a packaged game target",
            target.id
        )
        .into());
    }
    Ok(TargetManifest::new(vec![target.clone()]))
}

pub(crate) fn platform_eligibility(
    manifest: &TargetManifest,
    target: &str,
) -> Result<Vec<u8>, CliError> {
    let target = manifest
        .find(target)
        .ok_or_else(|| format!("target {target} is not defined"))?;
    Ok(serde_json::to_vec(&serde_json::json!({
        "schema": "astra.platform_eligibility.v1",
        "target": target.id,
        "profiles": target.default_profile.iter().collect::<Vec<_>>(),
        "platforms": target.platforms,
    }))?)
}
