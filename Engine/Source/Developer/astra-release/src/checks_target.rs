use super::*;

pub(crate) fn target_manifest_check(
    package: &PackageReader,
    selected: Option<&str>,
) -> ReleaseCheckRecord {
    let bytes = match package
        .container()
        .read_bounded("target.manifest", 256 * 1024)
    {
        Ok(bytes) => bytes,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "target.manifest".to_string(),
                domain: ReleaseDomain::Target,
                status: CheckStatus::Blocked,
                summary: "target manifest section could not be read".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_TARGET_MANIFEST",
                    err.to_string(),
                )),
                evidence: vec![evidence("section", "target.manifest")],
            };
        }
    };
    let manifest: TargetManifest = match serde_json::from_slice(&bytes) {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "target.manifest".to_string(),
                domain: ReleaseDomain::Target,
                status: CheckStatus::Blocked,
                summary: "target manifest is not valid JSON".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_TARGET_MANIFEST_JSON",
                    err.to_string(),
                )),
                evidence: vec![evidence("section", "target.manifest")],
            };
        }
    };
    let report = validate_manifest(&manifest, selected);
    let package_shape_diagnostic = package_target_shape_diagnostic(&manifest);
    let status = if package_shape_diagnostic.is_some() {
        CheckStatus::Blocked
    } else {
        match report.status {
            TargetValidationStatus::Pass => CheckStatus::Pass,
            TargetValidationStatus::Warning => CheckStatus::Warning,
            TargetValidationStatus::Blocked => CheckStatus::Blocked,
        }
    };
    ReleaseCheckRecord {
        id: "target.manifest".to_string(),
        domain: ReleaseDomain::Target,
        status,
        summary: "target manifest contains one packaged Game target".to_string(),
        diagnostic: package_shape_diagnostic.or_else(|| {
            report
                .diagnostics
                .iter()
                .find(|diagnostic| {
                    matches!(
                        diagnostic.severity,
                        astra_core::DiagnosticSeverity::Blocking
                            | astra_core::DiagnosticSeverity::Error
                    )
                })
                .cloned()
        }),
        evidence: vec![
            evidence("target_count", report.target_count),
            evidence("selected_target", selected.unwrap_or("")),
        ],
    }
}

pub(crate) fn headless_release_boundary_check(
    package: &PackageReader,
    selected: Option<&str>,
    profile: &str,
) -> ReleaseCheckRecord {
    let blocked = |code: &'static str, summary: String, evidence_values| ReleaseCheckRecord {
        id: "platform.headless_release_boundary".to_string(),
        domain: ReleaseDomain::Platform,
        status: CheckStatus::Blocked,
        summary: summary.clone(),
        diagnostic: Some(Diagnostic::blocking(code, summary)),
        evidence: evidence_values,
    };

    if let Some(entry) = package.container().entries().iter().find(|entry| {
        entry.schema == astra_platform::HEADLESS_HOST_PROFILE_SCHEMA
            || entry.schema.starts_with("astra.headless_")
    }) {
        return blocked(
            "ASTRA_HEADLESS_RELEASE_SCHEMA",
            "Headless schemas are test-only and cannot enter a release package".to_string(),
            vec![
                evidence("section", &entry.id),
                evidence("schema", &entry.schema),
            ],
        );
    }

    if package.has_section("platform.profiles") {
        match read_json_section(package, "platform.profiles") {
            Ok(value) if json_contains_headless_release_marker(&value) => {
                return blocked(
                    "ASTRA_HEADLESS_RELEASE_PROFILE",
                    "cooked platform profiles cannot contain a Headless launch profile".to_string(),
                    vec![evidence("section", "platform.profiles")],
                );
            }
            Ok(_) | Err(_) => {}
        }
    }

    if release_profile_requires_cooked_project(profile) {
        let manifest = package
            .container()
            .read_bounded("target.manifest", 256 * 1024)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<TargetManifest>(&bytes).ok());
        if let Some(target) = manifest.as_ref().and_then(|manifest| {
            selected
                .and_then(|selected| manifest.find(selected))
                .or_else(|| manifest.targets.first())
        }) {
            let has_headless_platform = target
                .platforms
                .iter()
                .any(|platform| platform.eq_ignore_ascii_case("headless"));
            let has_headless_binary = target.binary.as_deref().is_some_and(|binary| {
                matches!(binary, "astra-headless" | "astra_platform_headless")
            });
            let has_headless_crate = target.crate_name.as_deref().is_some_and(|crate_name| {
                matches!(crate_name, "astra-headless" | "astra-platform-headless")
            });
            if has_headless_platform || has_headless_binary || has_headless_crate {
                return blocked(
                    "ASTRA_HEADLESS_RELEASE_TARGET",
                    "shipping release targets cannot reference the Headless test host".to_string(),
                    vec![evidence("target", &target.id), evidence("profile", profile)],
                );
            }
        }
    }

    ReleaseCheckRecord {
        id: "platform.headless_release_boundary".to_string(),
        domain: ReleaseDomain::Platform,
        status: CheckStatus::Pass,
        summary: "release inputs are isolated from the Headless test host".to_string(),
        diagnostic: None,
        evidence: vec![evidence("profile", profile)],
    }
}

pub(crate) fn json_contains_headless_release_marker(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(value) => matches!(
            value.as_str(),
            "headless"
                | "astra-platform-headless"
                | "astra-headless"
                | astra_platform::HEADLESS_HOST_PROFILE_SCHEMA
        ),
        serde_json::Value::Array(values) => {
            values.iter().any(json_contains_headless_release_marker)
        }
        serde_json::Value::Object(values) => values.iter().any(|(key, value)| {
            matches!(key.as_str(), "headless" | "headless_profile")
                || json_contains_headless_release_marker(value)
        }),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            false
        }
    }
}

pub(crate) fn cooked_project_input_check(
    package: &PackageReader,
    profile: &str,
    manifest: Option<&PackageManifest>,
) -> ReleaseCheckRecord {
    let required = release_profile_requires_cooked_project(profile);
    let Some(entry) = package.container().section_entry("compiled.project") else {
        return if required {
            ReleaseCheckRecord {
                id: "package.cooked_project".to_string(),
                domain: ReleaseDomain::Package,
                status: CheckStatus::Blocked,
                summary: "release package must include a cooked project artifact".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_PACKAGE_COOKED_PROJECT_MISSING",
                    "release profile package validation requires compiled.project from cook/project input",
                )),
                evidence: vec![
                    evidence("section", "compiled.project"),
                    evidence("profile", profile),
                ],
            }
        } else {
            ReleaseCheckRecord {
                id: "package.cooked_project".to_string(),
                domain: ReleaseDomain::Package,
                status: CheckStatus::Pass,
                summary: "profile does not require a cooked project artifact".to_string(),
                diagnostic: None,
                evidence: vec![
                    evidence("section", "compiled.project"),
                    evidence("profile", profile),
                    evidence("required", required),
                ],
            }
        };
    };

    if entry.schema != "astra.cooked_project.v1" {
        return ReleaseCheckRecord {
            id: "package.cooked_project".to_string(),
            domain: ReleaseDomain::Package,
            status: CheckStatus::Blocked,
            summary: "compiled.project section uses an unexpected schema".to_string(),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_PACKAGE_COOKED_PROJECT_SCHEMA",
                format!(
                    "compiled.project schema must be astra.cooked_project.v1, got {}",
                    entry.schema
                ),
            )),
            evidence: vec![
                evidence("section", "compiled.project"),
                evidence("schema", &entry.schema),
            ],
        };
    }

    let bytes = match package
        .container()
        .read_bounded("compiled.project", 256 * 1024)
    {
        Ok(bytes) => bytes,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "package.cooked_project".to_string(),
                domain: ReleaseDomain::Package,
                status: CheckStatus::Blocked,
                summary: "compiled.project section could not be read".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_PACKAGE_COOKED_PROJECT_READ",
                    err.to_string(),
                )),
                evidence: vec![evidence("section", "compiled.project")],
            };
        }
    };
    let value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "package.cooked_project".to_string(),
                domain: ReleaseDomain::Package,
                status: CheckStatus::Blocked,
                summary: "compiled.project section is not valid JSON".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_PACKAGE_COOKED_PROJECT_JSON",
                    err.to_string(),
                )),
                evidence: vec![evidence("section", "compiled.project")],
            };
        }
    };
    for field in ["schema", "package_id", "profile", "target", "project_hash"] {
        if value
            .get(field)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .is_empty()
        {
            return ReleaseCheckRecord {
                id: "package.cooked_project".to_string(),
                domain: ReleaseDomain::Package,
                status: CheckStatus::Blocked,
                summary: "compiled.project section is missing required cook metadata".to_string(),
                diagnostic: Some(
                    Diagnostic::blocking(
                        "ASTRA_PACKAGE_COOKED_PROJECT_METADATA",
                        "compiled.project must record schema, package_id, profile, target and project_hash",
                    )
                    .with_field("field", field),
                ),
                evidence: vec![evidence("section", "compiled.project")],
            };
        }
    }
    if value.get("schema").and_then(serde_json::Value::as_str) != Some("astra.cooked_project.v1") {
        return ReleaseCheckRecord {
            id: "package.cooked_project".to_string(),
            domain: ReleaseDomain::Package,
            status: CheckStatus::Blocked,
            summary: "compiled.project payload schema is invalid".to_string(),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_PACKAGE_COOKED_PROJECT_PAYLOAD_SCHEMA",
                "compiled.project payload schema must be astra.cooked_project.v1",
            )),
            evidence: vec![evidence("section", "compiled.project")],
        };
    }

    if let Some(manifest) = manifest {
        if manifest.profile != profile {
            return ReleaseCheckRecord {
                id: "package.cooked_project".to_string(),
                domain: ReleaseDomain::Package,
                status: CheckStatus::Blocked,
                summary: "validation profile does not match package manifest".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_PACKAGE_PROFILE_MISMATCH",
                    "release validation profile must match package.manifest profile",
                )),
                evidence: vec![
                    evidence("section", "compiled.project"),
                    evidence("package_profile", &manifest.profile),
                    evidence("validation_profile", profile),
                ],
            };
        }
        let project_package_id = value
            .get("package_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let project_profile = value
            .get("profile")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if project_package_id != manifest.package_id || project_profile != manifest.profile {
            return ReleaseCheckRecord {
                id: "package.cooked_project".to_string(),
                domain: ReleaseDomain::Package,
                status: CheckStatus::Blocked,
                summary: "compiled.project metadata does not match package manifest".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_PACKAGE_COOKED_PROJECT_MISMATCH",
                    "compiled.project package_id and profile must match package.manifest",
                )),
                evidence: vec![
                    evidence("section", "compiled.project"),
                    evidence("package_profile", &manifest.profile),
                    evidence("cooked_profile", project_profile),
                ],
            };
        }
    }

    ReleaseCheckRecord {
        id: "package.cooked_project".to_string(),
        domain: ReleaseDomain::Package,
        status: CheckStatus::Pass,
        summary: "package includes a cooked project artifact from cook/project input".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", "compiled.project"),
            evidence("schema", "astra.cooked_project.v1"),
            evidence(
                "target",
                value
                    .get("target")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
            ),
            evidence("profile", profile),
        ],
    }
}

pub(crate) fn release_profile_requires_cooked_project(profile: &str) -> bool {
    matches!(profile, "release" | "desktop-release" | "web-release")
        || profile.ends_with("-release")
}

pub(crate) fn release_profile_requires_vn(profile: &str) -> bool {
    matches!(profile, "classic" | "modern")
        || VnAdvancedPresentationManifest::profile_requires_advanced(profile)
}
