use std::fs;

use astra_core::Hash256;
use astra_package::{
    PackageManifest, PackageReader, SourceFingerprintCryptoProvider, SourceUnlockPolicy,
    SourceVerificationManifest,
};
use astra_platform::PlatformId;
use astra_target::{validate_manifest, TargetManifest, TargetValidationStatus};

use super::*;

pub(crate) fn build_standalone_bundle_into(
    package: &std::path::Path,
    out: &std::path::Path,
    target: &str,
    profile: &str,
    platform: PlatformId,
    artifacts: &BundleArtifactInputs,
) -> Result<StandaloneBundleManifest, CliError> {
    let platform_name = platform_id_name(platform);
    let package_bytes = fs::read(package)?;
    let package_storage_hash = Hash256::from_sha256(&package_bytes).to_string();
    let raw_container = astra_package::AstraContainerReader::new(&package_bytes)?;
    let source_locked = raw_container.has_section("source.unlock");
    if source_locked && platform != PlatformId::Windows {
        return Err("source-locked private RC bundles are limited to Windows".into());
    }
    let source_profile_bundle_path = "source/source-profile.json";
    let source_profile_bytes = match (
        source_locked,
        artifacts.source_profile.as_deref(),
        artifacts.source_root.as_deref(),
    ) {
        (false, None, None) => None,
        (false, _, _) => {
            return Err("source profile/root require a source-locked package".into());
        }
        (true, Some(profile_path), Some(_)) => Some(fs::read(profile_path)?),
        (true, _, _) => {
            return Err("source-locked bundle requires --source-profile and --source-root".into());
        }
    };
    let reader = if source_locked {
        let policy: SourceUnlockPolicy = raw_container.decode_postcard("source.unlock")?;
        let profile: SourceVerificationManifest = serde_json::from_slice(
            source_profile_bytes
                .as_deref()
                .ok_or("source profile bytes are unavailable")?,
        )?;
        let mut source_reader = FilesystemSourceReader::open(
            artifacts
                .source_root
                .as_deref()
                .ok_or("source root is unavailable")?,
        )?;
        let crypto =
            SourceFingerprintCryptoProvider::unlock(&policy, &profile, &mut source_reader)?;
        PackageReader::open_source_locked(
            &package_bytes,
            &policy,
            "source.unlock",
            std::sync::Arc::new(crypto),
        )?
    } else {
        PackageReader::open(&package_bytes)?
    };
    if platform == PlatformId::Web && !astra_vn_package::load_policy_actions(&reader)?.is_empty() {
        return Err(
            "ASTRA_WEB_POLICY_ACTION_UNSUPPORTED: Web bundles cannot execute native policy actions"
                .into(),
        );
    }
    let package_manifest: PackageManifest =
        reader.container().decode_postcard("package.manifest")?;
    if !reader.has_section("platform.profiles") {
        return Err("standalone bundle requires cooked platform.profiles".into());
    }
    if package_manifest.profile != profile {
        return Err(format!(
            "bundle profile {profile} does not match package profile {}",
            package_manifest.profile
        )
        .into());
    }
    let target_manifest: TargetManifest =
        serde_json::from_slice(&reader.container().read_section("target.manifest")?)?;
    let target_report = validate_manifest(&target_manifest, Some(target));
    if matches!(target_report.status, TargetValidationStatus::Blocked) {
        return Err(format!("target validation failed: {target}").into());
    }
    let target_descriptor = target_manifest
        .find(target)
        .ok_or_else(|| format!("target {target} is not defined"))?;
    if !target_descriptor
        .platforms
        .iter()
        .any(|candidate| candidate == platform_name)
    {
        return Err(format!("target {target} does not support platform {platform_name}").into());
    }
    let display_config = package_player_display_config(&reader)?;
    let locale_config = package_player_locale_config(&reader)?;

    fs::create_dir_all(out.join("package"))?;
    let bundled_package = out.join("package").join("nativevn.astrapkg");
    fs::write(&bundled_package, &package_bytes)?;
    let scenario_bindings = if source_locked {
        Vec::new()
    } else {
        package_scenario_refs(&reader)?
    };
    let mut files = vec![bundle_file(
        "package/nativevn.astrapkg",
        "package",
        &package_bytes,
    )];
    if let Some(profile_bytes) = source_profile_bytes.as_deref() {
        let relative = validate_bundle_relative_path(source_profile_bundle_path)?;
        let destination = out.join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&destination, profile_bytes)?;
        files.push(bundle_file(
            source_profile_bundle_path,
            "source_verification_profile",
            profile_bytes,
        ));
    }
    let mut mount_policy = bundle_mount_policy(&reader, out, &mut files)?;
    let mut bundle_checks = Vec::new();
    let mut crash_reporter_ref = None;
    for scenario_ref in &scenario_bindings {
        let relative = validate_bundle_relative_path(&scenario_ref.path)?;
        let scenario_bytes = reader
            .container()
            .read_section(&scenario_ref.section_id)
            .map_err(|_| {
                format!(
                    "scenario ref {} section {} is not available in package",
                    scenario_ref.path, scenario_ref.section_id
                )
            })?;
        let destination = out.join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&destination, &scenario_bytes)?;
        files.push(bundle_file(
            &scenario_ref.path,
            "scenario_ref",
            &scenario_bytes,
        ));
    }

    let entrypoint = match platform {
        PlatformId::Windows => {
            let runtime = artifacts.windows_runtime.as_deref().ok_or(
                "Windows bundle requires --windows-runtime pointing to the matching Microsoft VC x64 CRT redistributable directory",
            )?;
            files.extend(windows_runtime::bundle(runtime, out)?);
            let entrypoint = "AstraPlayer.exe";
            let player_source = artifacts.windows_player.as_deref().ok_or(
                "Windows bundle requires --windows-player pointing to a built AstraPlayer.exe",
            )?;
            let exe_bytes = fs::read(player_source)?;
            let entrypoint_path = out.join(entrypoint);
            fs::write(&entrypoint_path, &exe_bytes)?;
            make_executable(&entrypoint_path)?;
            files.push(bundle_file(entrypoint, "windows_player", &exe_bytes));

            let reporter_name = "AstraCrashReporter.exe";
            let reporter_source = artifacts.crash_reporter.as_deref().ok_or(
                "Windows bundle requires --crash-reporter pointing to a built AstraCrashReporter.exe",
            )?;
            let reporter_bytes = fs::read(reporter_source)?;
            let reporter_self_test = std::process::Command::new(reporter_source)
                .arg("--self-test")
                .output()?;
            if !reporter_self_test.status.success() {
                return Err("AstraCrashReporter self-test failed".into());
            }
            let reporter_report: serde_json::Value =
                serde_json::from_slice(&reporter_self_test.stdout)?;
            if reporter_report
                .get("schema")
                .and_then(serde_json::Value::as_str)
                != Some("astra.crash_reporter_self_test.v1")
                || reporter_report
                    .get("status")
                    .and_then(serde_json::Value::as_str)
                    != Some("pass")
            {
                return Err("AstraCrashReporter self-test output is invalid".into());
            }
            fs::write(out.join(reporter_name), &reporter_bytes)?;
            files.push(bundle_file(
                reporter_name,
                "windows_crash_reporter",
                &reporter_bytes,
            ));
            crash_reporter_ref = Some(reporter_name.to_string());
            bundle_checks.push(PlayerLaunchCheck {
                id: "crash_reporter.packaged".to_string(),
                status: "pass".to_string(),
            });
            bundle_checks.push(PlayerLaunchCheck {
                id: "crash_reporter.self_test".to_string(),
                status: "pass".to_string(),
            });

            let config = player_config_bytes(PlayerConfigInput {
                target,
                profile,
                platform_name,
                package_storage_hash: &package_storage_hash,
                display_config: &display_config,
                locale_config: &locale_config,
                source_unlock_profile: source_locked.then_some(source_profile_bundle_path),
            })?;
            fs::write(out.join("AstraPlayer.config.json"), &config)?;
            files.push(bundle_file(
                "AstraPlayer.config.json",
                "player_config",
                &config,
            ));
            entrypoint.to_string()
        }
        PlatformId::Linux => {
            let entrypoint = "astra-player";
            let player_source = artifacts
                .linux_player
                .as_deref()
                .ok_or("Linux bundle requires --linux-player pointing to a built astra-player")?;
            let player_bytes = fs::read(player_source)?;
            let entrypoint_path = out.join(entrypoint);
            fs::write(&entrypoint_path, &player_bytes)?;
            make_executable(&entrypoint_path)?;
            files.push(bundle_file(entrypoint, "linux_player", &player_bytes));
            bundle_checks.push(PlayerLaunchCheck {
                id: "crash_reporter.not_applicable".to_string(),
                status: "pass".to_string(),
            });
            let config = player_config_bytes(PlayerConfigInput {
                target,
                profile,
                platform_name,
                package_storage_hash: &package_storage_hash,
                display_config: &display_config,
                locale_config: &locale_config,
                source_unlock_profile: source_locked.then_some(source_profile_bundle_path),
            })?;
            fs::write(out.join("AstraPlayer.config.json"), &config)?;
            files.push(bundle_file(
                "AstraPlayer.config.json",
                "player_config",
                &config,
            ));
            entrypoint.to_string()
        }
        PlatformId::Macos => {
            let player_source = artifacts.macos_player.as_deref().ok_or(
                "macOS bundle requires --macos-player pointing to a Universal 2 astra-player",
            )?;
            let player_bytes = fs::read(player_source)?;
            validate_universal_macho(&player_bytes)?;
            let contents = out.join("Contents");
            let macos = contents.join("MacOS");
            let resources = contents.join("Resources");
            fs::create_dir_all(&macos)?;
            fs::create_dir_all(&resources)?;
            fs::rename(out.join("package"), resources.join("package"))?;
            for scenario in &scenario_bindings {
                let relative = validate_bundle_relative_path(&scenario.path)?;
                let source = out.join(&relative);
                if source.is_file() {
                    let destination = resources.join(&relative);
                    if let Some(parent) = destination.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::rename(source, destination)?;
                }
            }
            if let Some(path) = mount_policy.as_mut() {
                let relative = validate_bundle_relative_path(path)?;
                let source = out.join(&relative);
                if source.is_file() {
                    let destination = resources.join(&relative);
                    if let Some(parent) = destination.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::rename(source, destination)?;
                }
                *path = format!("Contents/Resources/{path}");
            }
            for file in &mut files {
                file.path = format!("Contents/Resources/{}", file.path);
            }
            let entrypoint = "Contents/MacOS/astra-player";
            let entrypoint_path = out.join(entrypoint);
            fs::write(&entrypoint_path, &player_bytes)?;
            make_executable(&entrypoint_path)?;
            files.push(bundle_file(
                entrypoint,
                "macos_universal_player",
                &player_bytes,
            ));
            let config = player_config_bytes(PlayerConfigInput {
                target,
                profile,
                platform_name,
                package_storage_hash: &package_storage_hash,
                display_config: &display_config,
                locale_config: &locale_config,
                source_unlock_profile: source_locked.then_some(source_profile_bundle_path),
            })?;
            fs::write(resources.join("AstraPlayer.config.json"), &config)?;
            files.push(bundle_file(
                "Contents/Resources/AstraPlayer.config.json",
                "player_config",
                &config,
            ));
            let plist = macos_info_plist(&package_manifest.package_id);
            fs::write(contents.join("Info.plist"), plist.as_bytes())?;
            files.push(bundle_file(
                "Contents/Info.plist",
                "macos_info_plist",
                plist.as_bytes(),
            ));
            bundle_checks.extend([
                PlayerLaunchCheck {
                    id: "macos.universal2".into(),
                    status: "pass".into(),
                },
                PlayerLaunchCheck {
                    id: "macos.codesign".into(),
                    status: "required_external".into(),
                },
                PlayerLaunchCheck {
                    id: "macos.notarization".into(),
                    status: "required_external".into(),
                },
                PlayerLaunchCheck {
                    id: "crash_reporter.not_applicable".into(),
                    status: "pass".into(),
                },
            ]);
            entrypoint.to_string()
        }
        PlatformId::Web => {
            let entrypoint = "index.html";
            let index = br#"<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>AstraVN Player</title></head>
<body><canvas id="astra-player"></canvas><script type="module" src="astra-player-loader.js"></script></body>
</html>
"#;
            fs::write(out.join(entrypoint), index)?;
            files.push(bundle_file(entrypoint, "web_entrypoint", index));
            bundle_checks.push(PlayerLaunchCheck {
                id: "crash_reporter.not_applicable".to_string(),
                status: "pass".to_string(),
            });

            let config = player_config_bytes(PlayerConfigInput {
                target,
                profile,
                platform_name,
                package_storage_hash: &package_storage_hash,
                display_config: &display_config,
                locale_config: &locale_config,
                source_unlock_profile: source_locked.then_some(source_profile_bundle_path),
            })?;
            fs::write(out.join("AstraPlayer.config.json"), &config)?;
            files.push(bundle_file(
                "AstraPlayer.config.json",
                "player_config",
                &config,
            ));

            fs::write(out.join("astra-player-loader.js"), WEB_PLAYER_LOADER)?;
            files.push(bundle_file(
                "astra-player-loader.js",
                "web_player_loader",
                WEB_PLAYER_LOADER,
            ));
            fs::write(out.join("astra-audio-worklet.js"), WEB_AUDIO_WORKLET)?;
            files.push(bundle_file(
                "astra-audio-worklet.js",
                "web_audio_worklet",
                WEB_AUDIO_WORKLET,
            ));
            for (source, destination, role, missing) in [
                (
                    artifacts.web_player_wasm.as_deref(),
                    "astra_player_web_bg.wasm",
                    "web_player_wasm",
                    "Web bundle requires --web-player-wasm",
                ),
                (
                    artifacts.web_player_glue.as_deref(),
                    "astra_player_web.js",
                    "web_player_glue",
                    "Web bundle requires --web-player-glue",
                ),
            ] {
                let source = source.ok_or(missing)?;
                let bytes = fs::read(source)?;
                match role {
                    "web_player_wasm" => validate_web_player_wasm(&bytes)?,
                    "web_player_glue" => validate_web_player_glue(&bytes)?,
                    _ => unreachable!("web artifact role is closed"),
                }
                fs::write(out.join(destination), &bytes)?;
                files.push(bundle_file(destination, role, &bytes));
            }
            entrypoint.to_string()
        }
        PlatformId::Android => {
            let arm64 = artifacts.android_arm64_library.as_deref().ok_or(
                "Android bundle requires --android-arm64-library pointing to libastra_player_android.so",
            )?;
            let arm64_bytes = fs::read(arm64)?;
            validate_android_native_library(arm64, &arm64_bytes, "arm64-v8a")?;
            let entrypoint = "lib/arm64-v8a/libastra_player_android.so";
            let destination = out.join(entrypoint);
            fs::create_dir_all(destination.parent().expect("Android library has parent"))?;
            fs::write(&destination, &arm64_bytes)?;
            files.push(bundle_file(
                entrypoint,
                "android_player_cdylib",
                &arm64_bytes,
            ));

            if let Some(x86_64) = artifacts.android_x86_64_library.as_deref() {
                let bytes = fs::read(x86_64)?;
                validate_android_native_library(x86_64, &bytes, "x86_64")?;
                let relative = "lib/x86_64/libastra_player_android.so";
                let destination = out.join(relative);
                fs::create_dir_all(destination.parent().expect("Android library has parent"))?;
                fs::write(destination, &bytes)?;
                files.push(bundle_file(relative, "android_player_test_cdylib", &bytes));
            }
            let config = player_config_bytes(PlayerConfigInput {
                target,
                profile,
                platform_name,
                package_storage_hash: &package_storage_hash,
                display_config: &display_config,
                locale_config: &locale_config,
                source_unlock_profile: source_locked.then_some(source_profile_bundle_path),
            })?;
            fs::write(out.join("AstraPlayer.config.json"), &config)?;
            files.push(bundle_file(
                "AstraPlayer.config.json",
                "player_config",
                &config,
            ));
            bundle_checks.extend([
                PlayerLaunchCheck {
                    id: "android.arm64_cdylib.verified".to_string(),
                    status: "pass".to_string(),
                },
                PlayerLaunchCheck {
                    id: "android.apk_aab.pending_gradle".to_string(),
                    status: "blocking".to_string(),
                },
            ]);
            entrypoint.to_string()
        }
        PlatformId::Ios => {
            return Err(format!("standalone bundle platform {platform_name} is Stage 6").into());
        }
    };

    let manifest = StandaloneBundleManifest {
        schema: "astra.standalone_bundle_manifest.v2".to_string(),
        target: target.to_string(),
        profile: profile.to_string(),
        platform: platform_name.to_string(),
        entrypoint,
        package_hash: package_storage_hash,
        package: if platform == PlatformId::Macos {
            "Contents/Resources/package/nativevn.astrapkg".to_string()
        } else {
            "package/nativevn.astrapkg".to_string()
        },
        scenario_refs: scenario_bindings
            .into_iter()
            .map(|scenario| {
                if platform == PlatformId::Macos {
                    format!("Contents/Resources/{}", scenario.path)
                } else {
                    scenario.path
                }
            })
            .collect(),
        mount_policy,
        observability: BundleObservabilityEvidence {
            log_schema: astra_observability::LOG_EVENT_SCHEMA.to_string(),
            crash_reporting: if platform == PlatformId::Windows {
                "required".to_string()
            } else {
                "disabled".to_string()
            },
            crash_reporter: crash_reporter_ref,
        },
        checks: bundle_checks,
        files,
    };
    fs::write(
        out.join("bundle_manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(manifest)
}
