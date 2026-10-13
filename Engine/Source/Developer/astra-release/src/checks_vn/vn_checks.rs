use super::*;

pub(crate) fn vn_checks(
    package: &PackageReader,
    profile: &str,
    selected_target: Option<&str>,
) -> Vec<ReleaseCheckRecord> {
    if !release_profile_requires_vn(profile)
        && !package.has_section("vn.compiled_project")
        && !package.has_section("vn.profile_manifest")
    {
        return Vec::new();
    }
    let mut checks = vec![
        vn_compiled_project_check(package, profile),
        vn_locale_config_check(package),
        vn_profile_manifest_check(package, profile, selected_target),
        vn_policy_bundle_check(package, profile),
        vn_extension_bindings_check(package, profile),
        vn_standard_commands_check(package, profile),
        vn_presentation_provider_check(package, profile),
        vn_commercial_baseline_check(package, profile),
        vn_system_ui_profile_check(package, profile),
    ];
    if VnAdvancedPresentationManifest::profile_requires_advanced(profile) {
        checks.push(vn_advanced_presentation_check(package, profile));
    }
    checks
}

pub(crate) fn vn_locale_config_check(package: &PackageReader) -> ReleaseCheckRecord {
    let config = match load_player_locale_config(package) {
        Ok(config) => config,
        Err(error) => {
            return ReleaseCheckRecord {
                id: "player.locale_config".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "Player locale config or a declared localization table is invalid"
                    .to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_PLAYER_LOCALE_CONFIG",
                    error.to_string(),
                )),
                evidence: vec![evidence("section", "player.locale_config")],
            };
        }
    };
    let entry = package
        .container()
        .section_entry("player.locale_config")
        .expect("validated locale config section must exist");
    ReleaseCheckRecord {
        id: "player.locale_config".to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "Player locale config and every declared localization table are valid".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", "player.locale_config"),
            evidence("section_hash", entry.hash),
            evidence("default_locale", config.default_locale),
            evidence("locale_count", config.available_locales.len()),
        ],
    }
}

pub(crate) fn vn_compiled_project_check(
    package: &PackageReader,
    profile: &str,
) -> ReleaseCheckRecord {
    let compiled = match decode_compiled_project(package) {
        Ok(compiled) => compiled,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.compiled_project".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.compiled_project root or child section is missing or invalid"
                    .to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_COMPILED_PROJECT",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.compiled_project"),
                    evidence("profile", profile),
                ],
            };
        }
    };
    if compiled.schema != "astra.vn.compiled_project.v5"
        || compiled.story.stories.is_empty()
        || compiled.story.states.is_empty()
    {
        return ReleaseCheckRecord {
            id: "vn.compiled_project".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "compiled VN story does not contain runnable story/state data".to_string(),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_VN_COMPILED_STORY_SHAPE",
                "compiled VN story needs schema, stories and states",
            )),
            evidence: vec![evidence("section", "vn.compiled_project")],
        };
    }
    if compiled.story_manifest.schema != "astra.vn.story_manifest.v1"
        || compiled.variable_manifest.schema != "astra.vn.variable_manifest.v1"
        || compiled.command_manifest.schema != "astra.vn.command_manifest.v1"
        || compiled.story_manifest.stories.is_empty()
        || compiled.command_manifest.commands.is_empty()
        || compiled
            .command_manifest
            .commands
            .iter()
            .any(|command| command.source.is_none())
    {
        return ReleaseCheckRecord {
            id: "vn.compiled_project".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "compiled VN story is missing manifest/source-map evidence".to_string(),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_VN_COMPILED_STORY_MANIFEST",
                "compiled VN story must include story, variable and command manifests with command source refs",
            )),
            evidence: vec![
                evidence("section", "vn.compiled_project"),
                evidence("story_manifest_count", compiled.story_manifest.stories.len()),
                evidence("variable_scope_count", compiled.variable_manifest.scopes.len()),
                evidence("command_manifest_count", compiled.command_manifest.commands.len()),
            ],
        };
    }
    ReleaseCheckRecord {
        id: "vn.compiled_project".to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "compiled VN project root and all authoritative child sections are valid"
            .to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("story_hash", compiled.story_hash),
            evidence("story_count", compiled.stories.len()),
            evidence("state_count", compiled.states.len()),
            evidence("route_node_count", compiled.route_graph.nodes.len()),
            evidence(
                "command_manifest_count",
                compiled.command_manifest.commands.len(),
            ),
            evidence(
                "variable_scope_count",
                compiled.variable_manifest.scopes.len(),
            ),
        ],
    }
}

pub(crate) fn vn_profile_manifest_check(
    package: &PackageReader,
    profile: &str,
    selected_target: Option<&str>,
) -> ReleaseCheckRecord {
    let manifest = match package
        .container()
        .decode_postcard::<VnProfileManifest>("vn.profile_manifest")
    {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.profile_manifest".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.profile_manifest section is missing or invalid".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_PROFILE_MANIFEST",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.profile_manifest"),
                    evidence("profile", profile),
                ],
            };
        }
    };
    if manifest.schema != "astra.vn.profile_manifest.v1"
        || !manifest.profiles.iter().any(|p| p == profile)
    {
        return ReleaseCheckRecord {
            id: "vn.profile_manifest".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "VN profile manifest does not declare the validation profile".to_string(),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_VN_PROFILE_MISSING",
                "vn.profile_manifest must include the validation profile",
            )),
            evidence: vec![evidence("profile", profile)],
        };
    }
    if let Some(selected_target) = selected_target {
        if manifest.target != selected_target {
            return ReleaseCheckRecord {
                id: "vn.profile_manifest".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "VN profile manifest target does not match selected target".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_PROFILE_TARGET",
                    "vn.profile_manifest target must match selected target",
                )),
                evidence: vec![
                    evidence("manifest_target", manifest.target),
                    evidence("selected_target", selected_target),
                ],
            };
        }
    }
    ReleaseCheckRecord {
        id: "vn.profile_manifest".to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "VN profile manifest matches target and validation profile".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("target", manifest.target),
            evidence("profile", profile),
            evidence("profile_count", manifest.profiles.len()),
        ],
    }
}

pub(crate) fn vn_policy_bundle_check(package: &PackageReader, profile: &str) -> ReleaseCheckRecord {
    let manifest = match package
        .container()
        .decode_postcard::<VnPolicyBundleManifest>("vn.policy_bundle_manifest")
    {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.policy_bundle".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.policy_bundle_manifest section is missing or invalid".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_POLICY_BUNDLE",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.policy_bundle_manifest"),
                    evidence("profile", profile),
                ],
            };
        }
    };

    let cache = match package
        .container()
        .decode_postcard::<VnPolicyBundleSourceCache>("vn.policy_bundle_source_cache")
    {
        Ok(cache) => cache,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.policy_bundle".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.policy_bundle_source_cache section is missing or invalid".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_POLICY_CACHE",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.policy_bundle_source_cache"),
                    evidence("profile", profile),
                ],
            };
        }
    };

    let report = manifest.validate_standard_with_cache(&cache);
    if !report.passed {
        return ReleaseCheckRecord {
            id: "vn.policy_bundle".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary:
                "VN policy bundle manifest and source cache do not satisfy the standard policy gate"
                    .to_string(),
            diagnostic: report.diagnostics.first().cloned(),
            evidence: vec![
                evidence("section", "vn.policy_bundle_manifest"),
                evidence("source_cache_section", "vn.policy_bundle_source_cache"),
                evidence("profile", profile),
                evidence("bundle_count", manifest.bundles.len()),
                evidence("diagnostic_count", report.diagnostics.len()),
            ],
        };
    }

    ReleaseCheckRecord {
        id: "vn.policy_bundle".to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "VN standard policy bundle is present and locked".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", "vn.policy_bundle_manifest"),
            evidence("source_cache_section", "vn.policy_bundle_source_cache"),
            evidence("profile", profile),
            evidence("bundle_count", manifest.bundles.len()),
        ],
    }
}

pub(crate) fn vn_extension_bindings_check(
    package: &PackageReader,
    profile: &str,
) -> ReleaseCheckRecord {
    let manifest = match package
        .container()
        .decode_postcard::<VnExtensionManifest>("vn.extension_manifest")
    {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.extension_bindings".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.extension_manifest section is missing or invalid".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_EXTENSION_MANIFEST",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.extension_manifest"),
                    evidence("profile", profile),
                ],
            };
        }
    };

    let report = manifest.validate_required();
    if !report.passed {
        return ReleaseCheckRecord {
            id: "vn.extension_bindings".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "VN extension manifest is missing required provider bindings".to_string(),
            diagnostic: report.diagnostics.first().cloned(),
            evidence: vec![
                evidence("section", "vn.extension_manifest"),
                evidence("profile", profile),
                evidence("binding_count", manifest.bindings.len()),
                evidence("diagnostic_count", report.diagnostics.len()),
            ],
        };
    }

    ReleaseCheckRecord {
        id: "vn.extension_bindings".to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "VN extension manifest declares required explicit bindings".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", "vn.extension_manifest"),
            evidence("profile", profile),
            evidence("binding_count", manifest.bindings.len()),
        ],
    }
}

pub(crate) fn vn_standard_commands_check(
    package: &PackageReader,
    profile: &str,
) -> ReleaseCheckRecord {
    let manifest = match package
        .container()
        .decode_postcard::<VnStandardCommandManifest>("vn.standard_command_manifest")
    {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.standard_commands".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.standard_command_manifest section is missing or invalid".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_STANDARD_COMMAND_MANIFEST",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.standard_command_manifest"),
                    evidence("profile", profile),
                ],
            };
        }
    };
    let compiled = match decode_compiled_project(package) {
        Ok(compiled) => compiled,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.standard_commands".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.compiled_project is required to validate standard command usage"
                    .to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_COMPILED_STORY",
                    err.to_string(),
                )),
                evidence: vec![evidence("section", "vn.compiled_project")],
            };
        }
    };

    let report = manifest.validate_usage(&compiled);
    if !report.passed {
        return ReleaseCheckRecord {
            id: "vn.standard_commands".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "VN standard command manifest or compiled command usage failed validation"
                .to_string(),
            diagnostic: report.diagnostics.first().cloned(),
            evidence: vec![
                evidence("section", "vn.standard_command_manifest"),
                evidence("profile", profile),
                evidence("command_count", report.command_count),
                evidence("checked_usage_count", report.checked_usage_count),
                evidence("diagnostic_count", report.diagnostics.len()),
            ],
        };
    }

    ReleaseCheckRecord {
        id: "vn.standard_commands".to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "VN standard command manifest covers compiled presentation usage".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", "vn.standard_command_manifest"),
            evidence("profile", profile),
            evidence("command_count", report.command_count),
            evidence("checked_usage_count", report.checked_usage_count),
        ],
    }
}
