use astra_observability::{ConsoleFormat, CrashReportingMode, HostObservabilityConfig, HostRole};
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

pub const PLAYER_CONFIG_FILE: &str = "AstraPlayer.config.json";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundledObservabilityDescriptor {
    filter: String,
    console_format: ConsoleFormat,
    log_dir: String,
    crash_dir: String,
    crash_reporting: CrashReportingMode,
}

#[derive(Debug, Deserialize)]
struct PlayerBootstrapDescriptor {
    schema: String,
    observability: BundledObservabilityDescriptor,
}

#[derive(Debug, Default)]
pub struct PlayerObservabilityOverrides {
    pub filter: Option<String>,
    pub console_format: Option<ConsoleFormat>,
    pub log_dir: Option<PathBuf>,
    pub max_file_bytes: Option<usize>,
    pub max_archives: Option<usize>,
}

pub fn bundled_player_resource_root() -> Result<PathBuf, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("ASTRA_PLAYER_EXECUTABLE_PATH_UNAVAILABLE: {error}"))?;
    #[cfg(target_os = "macos")]
    {
        executable
            .parent()
            .and_then(Path::parent)
            .map(|contents| contents.join("Resources"))
            .ok_or_else(|| "ASTRA_PLAYER_RESOURCE_ROOT_UNAVAILABLE: malformed app bundle".into())
    }
    #[cfg(not(target_os = "macos"))]
    {
        executable.parent().map(Path::to_path_buf).ok_or_else(|| {
            "ASTRA_PLAYER_RESOURCE_ROOT_UNAVAILABLE: executable has no parent".into()
        })
    }
}

pub fn load_bundled_observability(
    resource_root: &Path,
    overrides: PlayerObservabilityOverrides,
) -> Result<HostObservabilityConfig, String> {
    let config_path = resource_root.join(PLAYER_CONFIG_FILE);
    let bytes = std::fs::read(&config_path)
        .map_err(|error| format!("ASTRA_PLAYER_CONFIG_READ_FAILED: {error}"))?;
    let descriptor: PlayerBootstrapDescriptor = serde_json::from_slice(&bytes)
        .map_err(|error| format!("ASTRA_PLAYER_CONFIG_INVALID: {error}"))?;
    if descriptor.schema != "astra.player_config.v2" {
        return Err(format!(
            "ASTRA_PLAYER_CONFIG_VERSION_UNSUPPORTED: {}",
            descriptor.schema
        ));
    }
    if descriptor.observability.filter.trim().is_empty() {
        return Err("ASTRA_PLAYER_OBSERVABILITY_FILTER_EMPTY".into());
    }
    let log_dir =
        resolve_bundled_relative_path(resource_root, &descriptor.observability.log_dir, "log_dir")?;
    let crash_dir = resolve_bundled_relative_path(
        resource_root,
        &descriptor.observability.crash_dir,
        "crash_dir",
    )?;
    let mut config = HostObservabilityConfig::for_cli(
        overrides.filter.unwrap_or(descriptor.observability.filter),
    );
    config.role = HostRole::Player;
    config.console_format = overrides
        .console_format
        .unwrap_or(descriptor.observability.console_format);
    config.log_dir = Some(overrides.log_dir.unwrap_or(log_dir));
    config.crash_dir = Some(crash_dir);
    config.crash_reporting = descriptor.observability.crash_reporting;
    if let Some(value) = overrides.max_file_bytes {
        config.max_file_bytes = value;
    }
    if let Some(value) = overrides.max_archives {
        config.max_archives = value;
    }
    Ok(config)
}

fn resolve_bundled_relative_path(
    resource_root: &Path,
    value: &str,
    field: &str,
) -> Result<PathBuf, String> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains("//")
        || value.contains('\\')
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "ASTRA_PLAYER_OBSERVABILITY_PATH_INVALID: {field} must be a safe relative path"
        ));
    }
    Ok(resource_root.join(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_config(root: &Path, log_dir: &str, crash_dir: &str) {
        std::fs::write(
            root.join(PLAYER_CONFIG_FILE),
            serde_json::json!({
                "schema": "astra.player_config.v2",
                "observability": {
                    "filter": "warn",
                    "console_format": "compact",
                    "log_dir": log_dir,
                    "crash_dir": crash_dir,
                    "crash_reporting": "required"
                }
            })
            .to_string(),
        )
        .unwrap();
    }

    #[test]
    fn loads_bundled_policy_and_applies_explicit_overrides() {
        let root = tempfile::tempdir().unwrap();
        write_config(root.path(), "Saved/Logs", "Saved/Crashes");
        let config = load_bundled_observability(
            root.path(),
            PlayerObservabilityOverrides {
                filter: Some("trace".into()),
                console_format: Some(ConsoleFormat::Json),
                ..PlayerObservabilityOverrides::default()
            },
        )
        .unwrap();
        assert_eq!(config.filter, "trace");
        assert_eq!(config.console_format, ConsoleFormat::Json);
        assert_eq!(config.log_dir, Some(root.path().join("Saved/Logs")));
        assert_eq!(config.crash_dir, Some(root.path().join("Saved/Crashes")));
        assert_eq!(config.crash_reporting, CrashReportingMode::Required);
    }

    #[test]
    fn rejects_paths_outside_the_bundle_root() {
        for invalid in ["../Logs", "/tmp/logs", "C:/logs", "Saved//Logs"] {
            let root = tempfile::tempdir().unwrap();
            write_config(root.path(), invalid, "Saved/Crashes");
            let error =
                load_bundled_observability(root.path(), PlayerObservabilityOverrides::default())
                    .unwrap_err();
            assert!(error.contains("ASTRA_PLAYER_OBSERVABILITY_PATH_INVALID"));
        }
    }
}
