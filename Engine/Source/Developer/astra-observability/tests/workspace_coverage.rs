use std::{collections::BTreeMap, fs, path::Path};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct CoverageFile {
    schema: String,
    crates: BTreeMap<String, CoverageEntry>,
}

#[derive(Debug, Deserialize)]
struct CoverageEntry {
    status: String,
    reason: String,
}

#[astra_headless_test::test]
fn every_workspace_member_has_enforced_observability_coverage() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let workspace: toml::Value =
        toml::from_str(&fs::read_to_string(root.join("Cargo.toml")).unwrap()).unwrap();
    let members = workspace["workspace"]["members"].as_array().unwrap();
    let coverage: CoverageFile =
        serde_json::from_slice(&fs::read(root.join("Docs/status/logging-coverage.json")).unwrap())
            .unwrap();
    assert_eq!(coverage.schema, "astra.logging_coverage.v1");
    assert_eq!(coverage.crates.len(), members.len());

    for member in members {
        let member = member.as_str().unwrap();
        let package: toml::Value =
            toml::from_str(&fs::read_to_string(root.join(member).join("Cargo.toml")).unwrap())
                .unwrap();
        let name = package["package"]["name"].as_str().unwrap();
        let entry = coverage
            .crates
            .get(name)
            .unwrap_or_else(|| panic!("workspace crate {name} is missing coverage classification"));
        assert!(
            !entry.reason.trim().is_empty(),
            "{name} needs a coverage reason"
        );
        match entry.status.as_str() {
            "instrumented" => {
                let manifest = fs::read_to_string(root.join(member).join("Cargo.toml")).unwrap();
                assert!(
                    manifest
                        .lines()
                        .any(|line| line.trim_start().starts_with("tracing")),
                    "instrumented crate {name} must depend on tracing"
                );
                let sources = rust_sources(&root.join(member).join("src"));
                assert!(
                    sources.iter().any(|source| contains_event(source)),
                    "instrumented crate {name} must emit a tracing event or span"
                );
            }
            "not_applicable" => {
                assert!(
                    entry.reason.contains("pure")
                        || entry.reason.contains("schema")
                        || entry.reason.contains("facade")
                        || entry.reason.contains("proc-macro"),
                    "not_applicable crate {name} needs a structural reason"
                );
            }
            status => panic!("unsupported coverage status {status} for {name}"),
        }
    }
}

fn rust_sources(root: &Path) -> Vec<String> {
    let mut output = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return output;
    };
    for entry in entries.filter_map(Result::ok) {
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            output.extend(rust_sources(&entry.path()));
        } else if entry.path().extension().and_then(|value| value.to_str()) == Some("rs") {
            output.push(fs::read_to_string(entry.path()).unwrap());
        }
    }
    output
}

fn contains_event(source: &str) -> bool {
    [
        "tracing::trace!",
        "tracing::debug!",
        "tracing::info!",
        "tracing::warn!",
        "tracing::error!",
        "trace!(",
        "debug!(",
        "info!(",
        "warn!(",
        "error!(",
        "info_span!(",
        "debug_span!(",
        "trace_span!(",
    ]
    .iter()
    .any(|needle| source.contains(needle))
}
