use super::*;

pub(crate) fn section_check(package: &PackageReader, section: &str) -> ReleaseCheckRecord {
    if package.has_section(section) {
        ReleaseCheckRecord {
            id: format!("{section}.present"),
            domain: match section {
                "media.manifest" => ReleaseDomain::Media,
                "target.manifest" => ReleaseDomain::Target,
                "platform.eligibility" => ReleaseDomain::Platform,
                "provider.policy" => ReleaseDomain::Plugin,
                _ => ReleaseDomain::Package,
            },
            status: CheckStatus::Pass,
            summary: format!("{section} section is present"),
            diagnostic: None,
            evidence: vec![evidence("section", section)],
        }
    } else {
        ReleaseCheckRecord {
            id: format!("{section}.present"),
            domain: ReleaseDomain::Package,
            status: CheckStatus::Blocked,
            summary: format!("{section} section is missing"),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_PACKAGE_SECTION_MISSING",
                format!("missing package section {section}"),
            )),
            evidence: vec![evidence("section", section)],
        }
    }
}

pub(crate) fn plugin_extension_registry_check(package: &PackageReader) -> ReleaseCheckRecord {
    let registry = match read_typed_json_section::<PluginExtensionRegistrySnapshot>(
        package,
        "plugin.extension_registry",
    ) {
        Ok(value) => value,
        Err((code, message)) => {
            return plugin_blocked(
                "plugin.extension_registry",
                code,
                message,
                vec![evidence("section", "plugin.extension_registry")],
            )
        }
    };
    let provider_policy =
        match read_typed_json_section::<ProviderPolicy>(package, "provider.policy") {
            Ok(value) => value,
            Err((code, message)) => {
                return plugin_blocked(
                    "plugin.extension_registry",
                    code,
                    message,
                    vec![evidence("section", "provider.policy")],
                )
            }
        };
    let manifest = match package
        .container()
        .decode_postcard::<PackageManifest>("package.manifest")
    {
        Ok(manifest) => manifest,
        Err(error) => {
            return plugin_blocked(
                "plugin.extension_registry",
                "ASTRA_PACKAGE_MANIFEST_INVALID",
                error.to_string(),
                vec![evidence("section", "package.manifest")],
            )
        }
    };
    if let Err(diagnostic) = registry.validate_embedded_package(
        &provider_policy,
        &manifest.package_id,
        &manifest.profile,
    ) {
        return plugin_blocked(
            "plugin.extension_registry",
            diagnostic.code,
            diagnostic.message,
            vec![evidence("section", "plugin.extension_registry")],
        );
    }

    ReleaseCheckRecord {
        id: "plugin.extension_registry".to_string(),
        domain: ReleaseDomain::Plugin,
        status: CheckStatus::Pass,
        summary: "plugin extension registry has resolved bindings and no conflicts".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("provider_count", registry.providers.len()),
            evidence("binding_count", registry.bindings.len()),
        ],
    }
}

pub(crate) fn plugin_dependency_graph_check(package: &PackageReader) -> ReleaseCheckRecord {
    let graph = match read_json_section(package, "plugin.dependency_graph") {
        Ok(value) => value,
        Err((code, message)) => {
            return plugin_blocked(
                "plugin.dependency_graph",
                code,
                message,
                vec![evidence("section", "plugin.dependency_graph")],
            )
        }
    };
    let dependencies = graph
        .get("dependencies")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(dependency) = dependencies.iter().find(|dependency| {
        dependency
            .get("required")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
            && !dependency
                .get("resolved")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
    }) {
        let plugin_id = dependency
            .get("plugin_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        return plugin_blocked(
            "plugin.dependency_graph",
            "ASTRA_PLUGIN_DEPENDENCY_UNRESOLVED",
            format!("required plugin dependency {plugin_id} is unresolved"),
            vec![evidence("plugin_id", plugin_id)],
        );
    }

    ReleaseCheckRecord {
        id: "plugin.dependency_graph".to_string(),
        domain: ReleaseDomain::Plugin,
        status: CheckStatus::Pass,
        summary: "plugin dependency graph has no unresolved required dependencies".to_string(),
        diagnostic: None,
        evidence: vec![evidence("dependency_count", dependencies.len())],
    }
}

pub(crate) fn product_context_checks(
    package: &PackageReader,
    selected_target: Option<&str>,
    profile: &str,
) -> Vec<ReleaseCheckRecord> {
    vec![
        product_context_binding_check(package, selected_target, profile),
        product_context_native_vn_check(package),
    ]
}

pub(crate) fn product_context_binding_check(
    package: &PackageReader,
    selected_target: Option<&str>,
    profile: &str,
) -> ReleaseCheckRecord {
    let provider_policy =
        match read_typed_json_section::<ProviderPolicy>(package, "provider.policy") {
            Ok(value) => value,
            Err((code, message)) => {
                return runtime_blocked(
                    "package.binding",
                    code,
                    message,
                    vec![evidence("section", "provider.policy")],
                )
            }
        };
    let registry = match read_typed_json_section::<PluginExtensionRegistrySnapshot>(
        package,
        "plugin.extension_registry",
    ) {
        Ok(value) => value,
        Err((code, message)) => {
            return runtime_blocked(
                "package.binding",
                code,
                message,
                vec![evidence("section", "plugin.extension_registry")],
            )
        }
    };
    let target_manifest = match package
        .container()
        .read_bounded("target.manifest", 256 * 1024)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<TargetManifest>(&bytes).ok())
    {
        Some(manifest) => manifest,
        None => {
            return runtime_blocked(
                "package.binding",
                "ASTRA_RUNTIME_PROVIDER_TARGET_MANIFEST",
                "runtime provider binding requires a valid target manifest",
                vec![evidence("section", "target.manifest")],
            )
        }
    };
    let Some(target) = select_game_target(&target_manifest, selected_target) else {
        return runtime_blocked(
            "package.binding",
            "ASTRA_RUNTIME_PROVIDER_TARGET_MISSING",
            "runtime provider binding requires one selected game target",
            vec![evidence("selected_target", selected_target.unwrap_or(""))],
        );
    };
    let manifest = match package
        .container()
        .decode_postcard::<PackageManifest>("package.manifest")
    {
        Ok(manifest) => manifest,
        Err(error) => {
            return runtime_blocked(
                "package.binding",
                "ASTRA_PACKAGE_MANIFEST_INVALID",
                error.to_string(),
                vec![evidence("section", "package.manifest")],
            )
        }
    };
    if let Err(diagnostic) =
        registry.validate_for_package(&provider_policy, &manifest.package_id, &target.id, profile)
    {
        return runtime_blocked(
            "package.binding",
            diagnostic.code,
            diagnostic.message,
            vec![evidence("target", &target.id), evidence("profile", profile)],
        );
    }
    ReleaseCheckRecord {
        id: "package.binding".to_string(),
        domain: ReleaseDomain::Runtime,
        status: CheckStatus::Pass,
        summary: "target manifest and explicit mechanism bindings match the package".to_string(),
        diagnostic: None,
        evidence: vec![evidence("target", &target.id)],
    }
}

pub(crate) fn product_context_native_vn_check(package: &PackageReader) -> ReleaseCheckRecord {
    let required_sections = [
        "vn.compiled_project",
        "vn.story",
        "vn.profile_manifest",
        "vn.policy_bundle_manifest",
        "vn.extension_manifest",
        "vn.standard_command_manifest",
        "vn.presentation_provider_manifest",
        "vn.commercial_baseline_manifest",
        "vn.system_story_manifest",
        "vn.system_ui_profile_manifest",
    ];
    for section in required_sections {
        if package
            .container()
            .read_bounded(section, 64 * 1024 * 1024)
            .is_err()
        {
            return runtime_blocked(
                "vn.runtime",
                "ASTRA_RUNTIME_PROVIDER_SECTION_DECLARATION",
                format!("NativeVN package must contain {section}"),
                vec![evidence("section", section)],
            );
        }
    }
    let behavioral_evidence = match native_vn_behavioral_evidence(package) {
        Ok(evidence) => evidence,
        Err((code, message)) => return runtime_blocked("vn.runtime", code, message, vec![]),
    };
    let mut release_evidence = vec![evidence("package_section_count", required_sections.len())];
    release_evidence.extend(behavioral_evidence);

    ReleaseCheckRecord {
        id: "vn.runtime".to_string(),
        domain: ReleaseDomain::Runtime,
        status: CheckStatus::Pass,
        summary: "NativeVN owned session completed package-bound save/restore and resume checks"
            .to_string(),
        diagnostic: None,
        evidence: release_evidence,
    }
}

pub(crate) fn native_vn_behavioral_evidence(
    package: &PackageReader,
) -> Result<Vec<ReleaseEvidence>, (&'static str, String)> {
    use astra_vn::{NativeVnStepCommand, NativeVnStepInput, VnSession, VnSessionConfig};
    let compiled = decode_compiled_project(package)
        .map_err(|error| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_PACKAGE", error.to_string()))?;
    let locale = load_player_locale_config(package)
        .map_err(|error| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_LOCALE", error.to_string()))?
        .default_locale;
    let selection = package.product_context();
    let mut session = VnSession::new(
        std::sync::Arc::new(compiled.story),
        astra_vn_core::VnRunConfig {
            profile: selection.profile().into(),
            locale,
        },
        VnSessionConfig {
            target_id: selection.target().into(),
            seed: 0xA57A,
            package: Some(astra_runtime::PackageHandle {
                package_id: package.package_hash().to_string(),
                target: selection.target().into(),
                ..Default::default()
            }),
            integrity_mode: astra_runtime::TickIntegrityMode::Evidence,
            worker_count: 4,
        },
    )
    .map_err(|error| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_OPEN", error.to_string()))?;
    let input = |fixed_step, mode, command| NativeVnStepInput {
        timing: astra_runtime::TickInput {
            fixed_step,
            delta_ns: 16_666_667,
            seed: 0xA57A,
        },
        mode,
        command,
    };
    let output = session
        .step(input(
            1,
            astra_runtime::TickMode::Live,
            NativeVnStepCommand::LaunchDefault,
        ))
        .map_err(|error| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_STEP", error.to_string()))?;
    let limits = VnHostLimits::new();
    limits
        .validate_output_count(
            output
                .presentations
                .len()
                .saturating_add(output.audio.len())
                .saturating_add(output.timeline.len()),
        )
        .map_err(|error| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_STEP", error.to_string()))?;
    let save = session
        .save()
        .map_err(|error| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_SAVE", error.to_string()))?;
    limits
        .validate_save_bytes(save.0.len())
        .map_err(|error| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_SAVE", error.to_string()))?;
    let report = session
        .restore(save.clone())
        .map_err(|error| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_RESTORE", error.to_string()))?;
    let restored = session
        .save()
        .map_err(|error| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_SAVE", error.to_string()))?;
    if restored != save || report.step != 1 || report.seed != 0xA57A {
        return Err((
            "ASTRA_RUNTIME_PROVIDER_BEHAVIOR_RESTORE_HASH",
            "native restore did not reproduce the saved Runtime container".into(),
        ));
    }
    let resumed = session
        .step(input(
            2,
            astra_runtime::TickMode::RestoreContinuation,
            NativeVnStepCommand::Execute(astra_vn_core::VnPlayerCommand::SetAuto { enabled: true }),
        ))
        .map_err(|error| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_RESUME", error.to_string()))?;
    if !resumed.vn_state.into_display_state().system.auto_enabled {
        return Err((
            "ASTRA_RUNTIME_PROVIDER_BEHAVIOR_RESUME",
            "restored native session did not apply typed input".into(),
        ));
    }
    session.close();
    Ok(vec![
        evidence(
            "behavior_state_hash",
            astra_core::Hash128::from_blake3(&save.0),
        ),
        evidence("behavior_save_bytes", save.0.len()),
        evidence("behavior_presentation_count", output.presentations.len()),
        evidence("behavior_restored_step", report.step),
        evidence("behavior_resumed_step", 2),
    ])
}

pub(crate) fn select_game_target<'a>(
    manifest: &'a TargetManifest,
    selected_target: Option<&str>,
) -> Option<&'a astra_target::TargetDescriptor> {
    if let Some(selected) = selected_target {
        return manifest
            .targets
            .iter()
            .find(|target| target.id == selected && target.kind == TargetKind::Game);
    }
    manifest
        .targets
        .iter()
        .find(|target| target.kind == TargetKind::Game)
}
