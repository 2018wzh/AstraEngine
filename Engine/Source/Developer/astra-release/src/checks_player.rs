use super::*;

pub(crate) fn player_presentation_check(
    report: Option<&PlayerPresentationReport>,
    package_hash: &str,
    target: Option<&str>,
    profile: &str,
    capability: Option<&PlatformCapabilityReport>,
    conformance: Option<&PlatformHostConformanceReport>,
    player: Option<&PlayerAutomationReport>,
) -> ReleaseCheckRecord {
    let player_evidence = |key: &str| {
        player
            .into_iter()
            .flat_map(|report| &report.checks)
            .flat_map(|check| &check.evidence)
            .find(|entry| entry.key == key)
            .map(|entry| entry.value.as_str())
    };
    let matches = report.is_some_and(|report| {
        let expected_platform = match report.platform {
            PlayerPlatform::Windows => astra_platform::PlatformId::Windows,
            PlayerPlatform::Linux => astra_platform::PlatformId::Linux,
            PlayerPlatform::Macos => astra_platform::PlatformId::Macos,
            PlayerPlatform::Web => astra_platform::PlatformId::Web,
            PlayerPlatform::Android => astra_platform::PlatformId::Android,
        };
        report.schema == PLAYER_PRESENTATION_REPORT_SCHEMA
            && report.status == PlayerAutomationStatus::Pass
            && report.package_hash == package_hash
            && report.profile == profile
            && target.is_none_or(|target| report.target == target)
            && report.presentation_path == "glyph_atlas"
            && report.layout_hash.starts_with("sha256:")
            && report.command_hash.starts_with("sha256:")
            && report.capture_hash.starts_with("sha256:")
            && report.font_provider_hash.starts_with("sha256:")
            && report.sequence > 0
            && report.width > 0
            && report.height > 0
            && report.changed_pixels > 0
            && report.diagnostics.is_empty()
            && capability
                .zip(conformance)
                .is_some_and(|(capability, conformance)| {
                    capability.platform == expected_platform
                        && capability.target == report.target
                        && capability.profile_hash == report.profile_hash
                        && capability.build_fingerprint == report.build_fingerprint
                        && capability.renderer.selected.as_deref()
                            == Some(report.renderer_provider.as_str())
                        && conformance.platform == expected_platform
                        && conformance.target == report.target
                        && conformance.profile_hash == report.profile_hash
                        && conformance.package_hash == report.package_hash
                        && conformance.build_fingerprint == report.build_fingerprint
                        && conformance.session_id == report.session_id
                })
            && player.is_some_and(|player| {
                player.package_hash == report.package_hash
                    && player.target == report.target
                    && player.profile == report.profile
                    && player_evidence("profile_hash") == Some(report.profile_hash.as_str())
                    && player_evidence("build_fingerprint")
                        == Some(report.build_fingerprint.as_str())
                    && player_evidence("session_id") == Some(report.session_id.as_str())
            })
    });
    ReleaseCheckRecord {
        id: "player.presentation".to_string(),
        domain: ReleaseDomain::Player,
        status: if matches {
            CheckStatus::Pass
        } else {
            CheckStatus::Blocked
        },
        summary: "live Player glyph presentation shares package, host and automation identity"
            .to_string(),
        diagnostic: (!matches).then(|| {
            Diagnostic::blocking(
                "ASTRA_PLAYER_PRESENTATION_EVIDENCE",
                "product release requires a live hardware glyph presentation report from the same run",
            )
        }),
        evidence: report
            .map(|report| {
                vec![
                    evidence("renderer_provider", &report.renderer_provider),
                    evidence("layout_hash", &report.layout_hash),
                    evidence("capture_hash", &report.capture_hash),
                    evidence("changed_pixels", report.changed_pixels),
                    evidence("session_id", &report.session_id),
                ]
            })
            .unwrap_or_default(),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn product_performance_check(
    evidence_set: &[ProductPerformanceEvidence],
    package_hash: &str,
    target: Option<&str>,
    profile: &str,
    capability: Option<&PlatformCapabilityReport>,
    conformance: Option<&PlatformHostConformanceReport>,
    player: Option<&PlayerAutomationReport>,
) -> ReleaseCheckRecord {
    let mut budget_ids = BTreeSet::new();
    let player_evidence = |key: &str| {
        player.and_then(|report| {
            report
                .checks
                .iter()
                .flat_map(|check| &check.evidence)
                .find(|entry| entry.key == key)
                .map(|entry| entry.value.as_str())
        })
    };
    let common_identity =
        capability
            .zip(conformance)
            .zip(player)
            .filter(|((capability, conformance), player)| {
                capability.profile_hash == conformance.profile_hash
                    && capability.build_fingerprint == conformance.build_fingerprint
                    && conformance.package_hash == package_hash
                    && player.package_hash == package_hash
                    && player.profile == profile
                    && target.is_none_or(|expected| {
                        capability.target == expected
                            && conformance.target == expected
                            && player.target == expected
                    })
                    && player_evidence("profile_hash") == Some(conformance.profile_hash.as_str())
                    && player_evidence("build_fingerprint")
                        == Some(conformance.build_fingerprint.as_str())
                    && player_evidence("session_id") == Some(conformance.session_id.as_str())
            });
    if evidence_set.is_empty() || common_identity.is_none() {
        return media_blocked(
            "performance.product_run",
            "ASTRA_PERFORMANCE_EVIDENCE_MISSING",
            "measured performance requires capability, host, player and budget evidence from one run",
            vec![evidence("report_count", evidence_set.len())],
        );
    }
    let ((capability, conformance), _) = common_identity.expect("identity presence was checked");
    for item in evidence_set {
        let identity = &item.report.identity;
        let valid = budget_ids.insert(item.budget.budget_id.clone())
            && validate_performance_report(&item.budget, identity, &item.report).is_ok()
            && !identity.dirty
            && identity.package_hash == package_hash
            && identity.profile == profile
            && identity.profile_hash == conformance.profile_hash
            && identity.build_fingerprint == conformance.build_fingerprint
            && identity.session_id == conformance.session_id
            && identity.target == capability.target
            && target.is_none_or(|expected| identity.target == expected);
        if !valid {
            return media_blocked(
                "performance.product_run",
                "ASTRA_PERFORMANCE_EVIDENCE_IDENTITY",
                "performance report failed its budget or same-run release identity validation",
                vec![
                    evidence("budget_id", &item.budget.budget_id),
                    evidence("package_hash", &identity.package_hash),
                    evidence("profile", &identity.profile),
                    evidence("target", &identity.target),
                ],
            );
        }
    }
    ReleaseCheckRecord {
        id: "performance.product_run".to_string(),
        domain: ReleaseDomain::Media,
        status: CheckStatus::Pass,
        summary: "performance budgets and measured reports share the product run identity"
            .to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("report_count", evidence_set.len()),
            evidence("package_hash", package_hash),
            evidence("profile_hash", &conformance.profile_hash),
            evidence("build_fingerprint", &conformance.build_fingerprint),
            evidence("session_id", &conformance.session_id),
        ],
    }
}

pub(crate) fn font_package_check(
    package: &PackageReader,
    selected_target: Option<&str>,
    profile: &str,
) -> ReleaseCheckRecord {
    let media = package
        .container()
        .read_bounded("media.manifest", 1024 * 1024)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let required = media
        .as_ref()
        .and_then(|value| value.get("font_manifest_required"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let section_id = media
        .as_ref()
        .and_then(|value| value.get("font_manifest_section"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("media.font_manifest");
    if !package.has_section(section_id) {
        if required {
            return media_blocked(
                "media.font_package",
                "ASTRA_TEXT_PACKAGE_MANIFEST_MISSING",
                "selected media profile requires a package font manifest",
                vec![evidence("section", section_id)],
            );
        }
        return ReleaseCheckRecord {
            id: "media.font_package".to_string(),
            domain: ReleaseDomain::Media,
            status: CheckStatus::Pass,
            summary: "media profile does not declare a package font manifest".to_string(),
            diagnostic: None,
            evidence: vec![evidence("required", false)],
        };
    }
    let Some(entry) = package.container().section_entry(section_id) else {
        unreachable!("package section presence was checked");
    };
    if entry.schema != FONT_PACKAGE_MANIFEST_SCHEMA {
        return media_blocked(
            "media.font_package",
            "ASTRA_TEXT_PACKAGE_MANIFEST_SCHEMA",
            "package font manifest uses an unsupported schema",
            vec![evidence("schema", &entry.schema)],
        );
    }
    let bytes = match package
        .container()
        .read_bounded(section_id, 4 * 1024 * 1024)
    {
        Ok(bytes) => bytes,
        Err(error) => {
            return media_blocked(
                "media.font_package",
                "ASTRA_TEXT_PACKAGE_MANIFEST_READ",
                format!("package font manifest could not be read: {error}"),
                vec![evidence("section", section_id)],
            )
        }
    };
    let manifest: FontPackageManifest = match serde_json::from_slice(&bytes) {
        Ok(manifest) => manifest,
        Err(error) => {
            return media_blocked(
                "media.font_package",
                "ASTRA_TEXT_PACKAGE_MANIFEST_DECODE",
                format!("package font manifest JSON is invalid: {error}"),
                vec![evidence("section", section_id)],
            )
        }
    };
    let target = selected_target
        .map(str::to_string)
        .unwrap_or_else(|| manifest.target.clone());
    let context = FontBindingContext {
        target: target.clone(),
        profile: profile.to_string(),
        default_locale: "und".to_string(),
    };
    if let Err(error) = astra_media::text_layout_from_package(
        package,
        section_id,
        context,
        TextLayoutConfig::production_defaults(),
    ) {
        return media_blocked(
            "media.font_package",
            "ASTRA_TEXT_PACKAGE_INVALID",
            error.to_string(),
            vec![
                evidence("section", section_id),
                evidence("target", target),
                evidence("profile", profile),
            ],
        );
    }
    ReleaseCheckRecord {
        id: "media.font_package".to_string(),
        domain: ReleaseDomain::Media,
        status: CheckStatus::Pass,
        summary: "package fonts resolve through the selected VFS/provider context".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", section_id),
            evidence("manifest_hash", Hash256::from_sha256(&bytes)),
            evidence("font_count", manifest.fonts.len()),
            evidence("target", target),
            evidence("profile", profile),
        ],
    }
}

pub(crate) fn cook_graph_identity_check(package: &PackageReader) -> ReleaseCheckRecord {
    let summary = package
        .container()
        .read_bounded("cook.summary", 1024 * 1024)
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            serde_json::from_slice::<CookSummaryManifest>(&bytes).map_err(|error| error.to_string())
        });
    match summary {
        Ok(summary) => ReleaseCheckRecord {
            id: "package.cook_graph".to_string(),
            domain: ReleaseDomain::Package,
            status: CheckStatus::Pass,
            summary: "cook graph and incremental artifact identity verified".to_string(),
            diagnostic: None,
            evidence: vec![
                evidence("graph_hash", summary.graph_hash),
                evidence("artifact_count", summary.artifact_count),
                evidence("cache_hit_count", summary.cache_hit_count),
                evidence("cooked_count", summary.cooked_count),
                evidence("max_concurrency", summary.max_concurrency),
            ],
        },
        Err(error) => ReleaseCheckRecord {
            id: "package.cook_graph".to_string(),
            domain: ReleaseDomain::Package,
            status: CheckStatus::Blocked,
            summary: "cook graph identity is unavailable".to_string(),
            diagnostic: Some(Diagnostic::blocking("ASTRA_COOK_GRAPH_IDENTITY", error)),
            evidence: Vec::new(),
        },
    }
}

pub(crate) fn platform_conformance_check(
    capability: Option<&PlatformCapabilityReport>,
    conformance: Option<&PlatformHostConformanceReport>,
    package_hash: &str,
    required: bool,
) -> ReleaseCheckRecord {
    let (Some(capability), Some(conformance)) = (capability, conformance) else {
        return ReleaseCheckRecord {
            id: "platform.host_conformance".to_string(),
            domain: ReleaseDomain::Platform,
            status: if required {
                CheckStatus::Blocked
            } else {
                CheckStatus::Warning
            },
            summary: "platform host conformance report was not supplied".to_string(),
            diagnostic: Some(if required {
                Diagnostic::blocking(
                    "ASTRA_PLATFORM_CONFORMANCE_MISSING",
                    "a host conformance report is required for this release profile",
                )
            } else {
                Diagnostic::warning(
                    "ASTRA_PLATFORM_CONFORMANCE_MISSING",
                    "host conformance evidence is pending",
                )
            }),
            evidence: Vec::new(),
        };
    };
    let (validation, diagnostics) =
        astra_platform::validate_conformance_report(capability, conformance);
    let package_matches = conformance.package_hash == package_hash;
    let status = if validation == PlatformValidationStatus::Pass && package_matches {
        CheckStatus::Pass
    } else {
        CheckStatus::Blocked
    };
    ReleaseCheckRecord {
        id: "platform.host_conformance".to_string(),
        domain: ReleaseDomain::Platform,
        status,
        summary: "host conformance evidence is bound to the release identity".to_string(),
        diagnostic: if status == CheckStatus::Pass {
            None
        } else {
            Some(diagnostics.first().cloned().unwrap_or_else(|| {
                Diagnostic::blocking(
                    "ASTRA_PLATFORM_CONFORMANCE_PACKAGE_IDENTITY",
                    "host conformance package hash does not match the release package",
                )
            }))
        },
        evidence: vec![
            evidence("profile_hash", &conformance.profile_hash),
            evidence("package_hash", &conformance.package_hash),
            evidence("build_fingerprint", &conformance.build_fingerprint),
            evidence("session_id", &conformance.session_id),
            evidence("check_count", conformance.checks.len()),
        ],
    }
}

pub(crate) fn platform_evidence_continuity_check(
    capability: Option<&PlatformCapabilityReport>,
    conformance: Option<&PlatformHostConformanceReport>,
    player: &PlayerAutomationReport,
) -> ReleaseCheckRecord {
    let player_evidence = |key: &str| {
        player
            .checks
            .iter()
            .flat_map(|check| &check.evidence)
            .find(|entry| entry.key == key)
            .map(|entry| entry.value.as_str())
    };
    let matches = capability
        .zip(conformance)
        .is_some_and(|(capability, conformance)| {
            conformance.profile_hash == capability.profile_hash
                && conformance.build_fingerprint == capability.build_fingerprint
                && conformance.package_hash == player.package_hash
                && player_evidence("profile_hash") == Some(conformance.profile_hash.as_str())
                && player_evidence("build_fingerprint")
                    == Some(conformance.build_fingerprint.as_str())
                && player_evidence("session_id") == Some(conformance.session_id.as_str())
        });
    ReleaseCheckRecord {
        id: "platform.evidence_continuity".to_string(),
        domain: ReleaseDomain::Platform,
        status: if matches {
            CheckStatus::Pass
        } else {
            CheckStatus::Blocked
        },
        summary: "capability, host conformance and player automation identities are continuous"
            .to_string(),
        diagnostic: (!matches).then(|| {
            Diagnostic::blocking(
                "ASTRA_PLATFORM_EVIDENCE_CONTINUITY",
                "platform reports do not share profile, package, build and session identity",
            )
        }),
        evidence: player_evidence("session_id")
            .map(|session_id| vec![evidence("session_id", session_id)])
            .unwrap_or_default(),
    }
}

pub(crate) fn release_status(checks: &[ReleaseCheckRecord]) -> CheckStatus {
    if checks
        .iter()
        .any(|check| check.status == CheckStatus::Blocked)
    {
        CheckStatus::Blocked
    } else if checks
        .iter()
        .any(|check| check.status == CheckStatus::Warning)
    {
        CheckStatus::Warning
    } else {
        CheckStatus::Pass
    }
}

pub(crate) fn player_full_playable_check(
    report: &PlayerAutomationReport,
    package_hash: &str,
    profile: &str,
    target: Option<&str>,
) -> ReleaseCheckRecord {
    let target_matches = target.is_none_or(|target| report.target == target);
    if report.schema == "astra.player_automation_report.v1"
        && report.status == PlayerAutomationStatus::Pass
        && report.full_playable_passed()
        && report.package_hash == package_hash
        && report.profile == profile
        && target_matches
        && report.transcript_hash.starts_with("sha256:")
        && !report.route_coverage.is_empty()
    {
        ReleaseCheckRecord {
            id: "player.full_playable".to_string(),
            domain: ReleaseDomain::Player,
            status: CheckStatus::Pass,
            summary: "live player automation report proves full playable route coverage"
                .to_string(),
            diagnostic: None,
            evidence: vec![
                evidence("package_hash", package_hash),
                evidence("profile", profile),
                evidence("target", &report.target),
                evidence("transcript_hash", &report.transcript_hash),
                evidence("route_count", report.route_coverage.len()),
            ],
        }
    } else {
        ReleaseCheckRecord {
            id: "player.full_playable".to_string(),
            domain: ReleaseDomain::Player,
            status: CheckStatus::Blocked,
            summary: "live player automation report is missing or does not match package identity"
                .to_string(),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_PLAYER_FULL_PLAYABLE_EVIDENCE",
                "player.full_playable requires a passing live automation report for this package",
            )),
            evidence: vec![
                evidence("expected_package_hash", package_hash),
                evidence("actual_package_hash", &report.package_hash),
                evidence("expected_profile", profile),
                evidence("actual_profile", &report.profile),
                evidence("route_count", report.route_coverage.len()),
            ],
        }
    }
}
