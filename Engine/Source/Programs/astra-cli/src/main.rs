mod nativevn_actions;
mod windows_runtime;

use std::{fs, path::Path};

use astra_asset::VfsUri;
use astra_cook::CookCancellationToken;
use astra_core::Hash256;
use astra_release::{PackageValidateRequest, ReleaseValidator};
use astra_target::validate_manifest;
use astra_vn_script::{
    compile_astra_project, format_astra_source, AstraSource, CompileAstraProjectOptions,
    FormatOptions,
};
use clap::Parser;
use serde::Deserialize;
use tracing::info;

mod args;
mod bundle_package;
mod bundle_standalone;
mod bundle_validate;
mod cook_nativevn;
mod cook_paths;
mod cook_project;
mod cook_types;
mod report;
mod ui;

pub(crate) use args::*;
pub(crate) use bundle_package::*;
pub(crate) use bundle_standalone::*;
pub(crate) use bundle_validate::*;
pub(crate) use cook_nativevn::*;
pub(crate) use cook_paths::*;
pub(crate) use cook_project::*;
pub(crate) use cook_types::*;
pub(crate) use report::*;
pub(crate) use ui::*;

pub(crate) fn main() -> Result<(), CliError> {
    let cli = Cli::parse();
    let _log_guard = init_logging(&cli)?;
    match cli.command {
        Command::Cook {
            project,
            profile,
            target,
            out,
        } => {
            let cancellation = CookCancellationToken::default();
            let signal_token = cancellation.clone();
            ctrlc::set_handler(move || signal_token.cancel())?;
            let manifest = cook_project(project, &profile, target.as_deref(), out, &cancellation)?;
            println!("{}", serde_yaml::to_string(&manifest)?);
        }
        Command::Ui { command } => run_ui_command(command)?,
        Command::Script { command } => match command {
            ScriptCommand::Check { sources } => {
                let sources = read_astra_sources(&sources)?;
                let compiled =
                    compile_astra_project(sources, CompileAstraProjectOptions::default())?;
                println!(
                    "{}",
                    serde_json::json!({
                        "schema": "astra.script_check_report.v1",
                        "status": "pass",
                        "story_hash": compiled.story_hash.to_string(),
                        "command_count": compiled.command_manifest.commands.len()
                    })
                );
            }
            ScriptCommand::Format {
                sources,
                check,
                write,
            } => {
                if !check && !write {
                    return Err("astra script format requires --check or --write".into());
                }
                let mut candidates = Vec::new();
                for path in &sources {
                    let source = fs::read_to_string(path)?;
                    let formatted = format_astra_source(
                        path.to_string_lossy(),
                        &source,
                        FormatOptions::default(),
                    )?;
                    if formatted != source {
                        candidates.push((path.clone(), source, formatted));
                    }
                }
                if check && !candidates.is_empty() {
                    return Err(format!(
                        "ASTRA_SCRIPT_FORMAT_REQUIRED: {} source file(s) require formatting",
                        candidates.len()
                    )
                    .into());
                }
                if write && !candidates.is_empty() {
                    let original = sources
                        .iter()
                        .map(|path| {
                            Ok(AstraSource::story(
                                path.to_string_lossy(),
                                fs::read_to_string(path)?,
                            ))
                        })
                        .collect::<Result<Vec<_>, std::io::Error>>()?;
                    let formatted = original
                        .iter()
                        .map(|source| {
                            candidates
                                .iter()
                                .find(|(path, _, _)| path.to_string_lossy() == source.path)
                                .map_or_else(
                                    || source.clone(),
                                    |(_, _, text)| {
                                        AstraSource::story(source.path.clone(), text.clone())
                                    },
                                )
                        })
                        .collect::<Vec<_>>();
                    let before = compile_astra_project(original, Default::default())?;
                    let after = compile_astra_project(formatted, Default::default())?;
                    if before.story_hash != after.story_hash {
                        return Err("ASTRA_SCRIPT_FORMAT_SEMANTIC_CHANGE: formatter changed compiled semantics".into());
                    }
                    for (path, _, formatted) in &candidates {
                        atomic_replace(path, formatted.as_bytes())?;
                    }
                }
                println!(
                    "{}",
                    serde_json::json!({
                        "schema": "astra.script_format_report.v1",
                        "status": "pass",
                        "changed": candidates.len(),
                        "mode": if write { "write" } else { "check" }
                    })
                );
            }
        },
        Command::Package { command } => match command {
            PackageCommand::Build {
                cooked,
                out,
                target,
                source_unlock_policy,
                source_profile,
                source_root,
            } => {
                let manifest = read_cook_manifest(&cooked)?;
                let source_lock = SourceLockBuildInputs::from_paths(
                    source_unlock_policy.as_deref(),
                    source_profile.as_deref(),
                    source_root.as_deref(),
                )?;
                let package = build_package_from_cooked(
                    &cooked,
                    manifest,
                    target.as_deref(),
                    source_lock.as_ref(),
                )?;
                if let Some(parent) = out.parent() {
                    fs::create_dir_all(parent)?;
                }
                atomic_replace(&out, &package.into_bytes())?;
            }
            PackageCommand::Bundle(args) => {
                let PackageBundleArgs {
                    package,
                    out,
                    target,
                    profile,
                    platform,
                    windows_player,
                    windows_runtime,
                    linux_player,
                    macos_player,
                    crash_reporter,
                    web_player_wasm,
                    web_player_glue,
                    android_arm64_library,
                    android_x86_64_library,
                    source_profile,
                    source_root,
                    format,
                } = *args;
                let artifacts = BundleArtifactInputs {
                    windows_player,
                    windows_runtime,
                    linux_player,
                    macos_player,
                    crash_reporter,
                    web_player_wasm,
                    web_player_glue,
                    android_arm64_library,
                    android_x86_64_library,
                    source_profile,
                    source_root,
                };
                let manifest = build_standalone_bundle(
                    &package,
                    &out,
                    &target,
                    &profile,
                    platform.into(),
                    &artifacts,
                )?;
                println!("{}", encode_bundle_manifest(&manifest, format)?);
            }
            PackageCommand::Validate {
                package,
                profile,
                target,
                platform_report,
                platform_conformance_report,
                report,
                player_automation_report,
                format,
            } => {
                let bytes = fs::read(package)?;
                let platform_report = read_platform_report(platform_report.as_deref())?;
                let platform_conformance_report =
                    read_platform_conformance_report(platform_conformance_report.as_deref())?;
                let player_report =
                    read_player_automation_report(player_automation_report.as_deref())?;
                let require_platform_report = release_profile_requires_platform_report(&profile);
                let release_report = ReleaseValidator.validate_package_with_platform_evidence(
                    PackageValidateRequest {
                        package_bytes: bytes,
                        profile,
                        require_ffmpeg: false,
                        target,
                        require_platform_report,
                        platform_report,
                    },
                    platform_conformance_report,
                    player_report,
                )?;
                let encoded = encode_release_report(&release_report, format)?;
                if let Some(path) = report {
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(path, &encoded)?;
                } else {
                    println!("{encoded}");
                }
            }
        },
        Command::Report {
            command: ReportCommand::Explain { report },
        } => {
            info!("cli.report.explain");
            let text = fs::read_to_string(report)?;
            println!("{}", explain_report(&text)?);
        }
        Command::Target { command } => match command {
            TargetCommand::List { project, format } => {
                let manifest = read_target_manifest(&project)?;
                println!("{}", encode_target_manifest(&manifest, format)?);
            }
            TargetCommand::Validate {
                project,
                target,
                format,
            } => {
                let manifest = read_target_manifest(&project)?;
                let report = validate_manifest(&manifest, target.as_deref());
                println!("{}", encode_target_report(&report, format)?);
            }
        },
        Command::Platform { command } => match command {
            PlatformCommand::Probe {
                platform,
                target,
                report,
                format,
            } => {
                let platform_report = probe_platform(platform.into(), target.as_deref());
                let encoded = encode_platform_report(&platform_report, format)?;
                if let Some(path) = report {
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(path, &encoded)?;
                } else {
                    println!("{encoded}");
                }
            }
        },
    }
    Ok(())
}

#[cfg(test)]
mod ui_cli_tests {
    use super::*;
    use astra_core::Hash256;
    use astra_package::{PackageBuildRequest, SectionPayload, SourceUnlockPolicy};

    #[test]
    fn matrix_size_parser_enforces_the_preview_contract() {
        assert_eq!(parse_ui_matrix_size("1920x1080").unwrap(), (1920, 1080));
        for invalid in ["1920", "0x720", "8193x720", "640x8193", "640X480"] {
            assert!(parse_ui_matrix_size(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn scene_evidence_contains_only_counts_and_the_precomputed_hash() {
        let commands = vec![
            astra_media::SceneCommand::rect("panel", 1, 2, 3, 4, [5, 6, 7, 8]),
            astra_media::SceneCommand::Clear {
                rgba: [9, 10, 11, 12],
            },
        ];
        let evidence = ui_scene_evidence(&commands, "sha256:scene").unwrap();
        let encoded = serde_json::to_value(evidence).unwrap();
        assert_eq!(encoded["schema"], "astra.ui_scene_evidence.v1");
        assert_eq!(encoded["scene_hash"], "sha256:scene");
        assert_eq!(encoded["command_count"], 2);
        assert_eq!(encoded["command_counts"]["rect"], 1);
        assert_eq!(encoded["command_counts"]["clear"], 1);
        for forbidden in ["rgba", "glyphs", "frame", "vertices", "indices"] {
            assert!(!encoded.to_string().contains(forbidden));
        }
    }

    #[test]
    fn macos_bundle_requires_both_universal_slices() {
        let mut binary = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 2];
        for cpu in [0x01000007_u32, 0x0100000c] {
            binary.extend_from_slice(&cpu.to_be_bytes());
            binary.extend_from_slice(&[0; 16]);
        }
        assert!(validate_universal_macho(&binary).is_ok());
        binary[7] = 1;
        assert!(validate_universal_macho(&binary).is_err());
    }

    #[test]
    fn source_lock_policy_must_cover_every_product_payload_section() {
        let request = PackageBuildRequest::fixture(
            "com.example.locked",
            "classic",
            vec![SectionPayload::raw(
                "vn.story",
                "astra.vn.compiled_project.v6",
                vec![1, 2, 3],
            )],
        );
        let mut policy = SourceUnlockPolicy {
            schema: "astra.source_unlock_policy.v1".into(),
            source_profile: "fixture.original.v1".into(),
            verification_manifest_hash: Hash256::from_sha256(b"manifest"),
            crypto_provider: astra_package::SOURCE_FINGERPRINT_PROVIDER_ID.into(),
            protected_sections: std::collections::BTreeSet::new(),
            max_files: 1,
            max_file_bytes: 1024,
            max_total_bytes: 1024,
        };
        assert!(validate_source_lock_coverage(&request, &policy).is_err());
        policy.protected_sections.extend(
            [
                "vn.story",
                "cook.summary",
                "asset.vfs_manifest",
                "asset.catalog",
                "media.manifest",
                "scenario.refs",
            ]
            .into_iter()
            .map(str::to_string),
        );
        validate_source_lock_coverage(&request, &policy).unwrap();
    }
}
