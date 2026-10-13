use super::*;

pub(crate) fn vn_presentation_provider_check(
    package: &PackageReader,
    profile: &str,
) -> ReleaseCheckRecord {
    let manifest = match load_presentation_provider_manifest(package, profile) {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.presentation_provider".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.presentation_provider_manifest section is missing or invalid"
                    .to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_PRESENTATION_PROVIDER_MANIFEST",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.presentation_provider_manifest"),
                    evidence("profile", profile),
                ],
            };
        }
    };

    let report = manifest.validate_standard();
    if !report.passed {
        return ReleaseCheckRecord {
            id: "vn.presentation_provider".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary:
                "VN presentation provider manifest lacks required filter/fallback/wait support"
                    .to_string(),
            diagnostic: report.diagnostics.first().cloned(),
            evidence: vec![
                evidence("section", "vn.presentation_provider_manifest"),
                evidence("profile", profile),
                evidence("filter_count", report.filter_count),
                evidence("fallback_count", report.fallback_count),
                evidence("wait_capability_count", report.wait_capability_count),
                evidence("profile_count", report.profile_count),
                evidence("preset_count", report.preset_count),
                evidence("diagnostic_count", report.diagnostics.len()),
            ],
        };
    }

    ReleaseCheckRecord {
        id: "vn.presentation_provider".to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary:
            "VN presentation provider declares validated profile, preset, fallback and await policy"
                .to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", "vn.presentation_provider_manifest"),
            evidence("profile", profile),
            evidence("filter_count", report.filter_count),
            evidence("fallback_count", report.fallback_count),
            evidence("wait_capability_count", report.wait_capability_count),
            evidence("profile_count", report.profile_count),
            evidence("preset_count", report.preset_count),
        ],
    }
}

pub(crate) fn vn_commercial_baseline_check(
    package: &PackageReader,
    profile: &str,
) -> ReleaseCheckRecord {
    let manifest = match package
        .container()
        .decode_postcard::<VnCommercialBaselineManifest>("vn.commercial_baseline_manifest")
    {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.commercial_baseline".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.commercial_baseline_manifest section is missing or invalid"
                    .to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_COMMERCIAL_BASELINE_MANIFEST",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.commercial_baseline_manifest"),
                    evidence("profile", profile),
                ],
            };
        }
    };

    let report = manifest.validate_required();
    if !report.passed {
        return ReleaseCheckRecord {
            id: "vn.commercial_baseline".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "VN commercial baseline is missing required automated feature evidence"
                .to_string(),
            diagnostic: report.diagnostics.first().cloned(),
            evidence: vec![
                evidence("section", "vn.commercial_baseline_manifest"),
                evidence("profile", profile),
                evidence("story_hash", manifest.story_hash),
                evidence("required_count", report.required_count),
                evidence("feature_count", report.feature_count),
                evidence("diagnostic_count", report.diagnostics.len()),
            ],
        };
    }

    ReleaseCheckRecord {
        id: "vn.commercial_baseline".to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "VN commercial baseline declares required automated feature evidence".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", "vn.commercial_baseline_manifest"),
            evidence("profile", profile),
            evidence("story_hash", manifest.story_hash),
            evidence("required_count", report.required_count),
            evidence("feature_count", report.feature_count),
        ],
    }
}

pub(crate) fn vn_system_ui_profile_check(
    package: &PackageReader,
    profile: &str,
) -> ReleaseCheckRecord {
    let manifest = match package
        .container()
        .decode_postcard::<SystemStoryManifest>("vn.system_story_manifest")
    {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.system_ui_profile".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.system_story_manifest section is missing or invalid".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_SYSTEM_MANIFEST",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.system_story_manifest"),
                    evidence("profile", profile),
                ],
            };
        }
    };
    if manifest.schema != "astra.vn.system_story_manifest.v2" {
        return ReleaseCheckRecord {
            id: "vn.system_ui_profile".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "VN system story manifest has an unexpected schema".to_string(),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_VN_SYSTEM_MANIFEST_SCHEMA",
                "vn.system_story_manifest must use astra.vn.system_story_manifest.v2",
            )),
            evidence: vec![evidence("section", "vn.system_story_manifest")],
        };
    }
    let required = SystemStoryManifest::commercial_required_pages();
    let report = manifest.validate_required(&required);
    if report.status == SystemStoryValidationStatus::Blocked {
        let missing_count = report.diagnostics.len();
        return ReleaseCheckRecord {
            id: "vn.system_ui_profile".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "VN system UI profile is missing required system story entries".to_string(),
            diagnostic: report.diagnostics.first().cloned(),
            evidence: vec![
                evidence("section", "vn.system_story_manifest"),
                evidence("profile", profile),
                evidence("page_count", manifest.entries.len()),
                evidence("required_count", required.len()),
                evidence("missing_count", missing_count),
            ],
        };
    }

    let profile_manifest = match package
        .container()
        .decode_postcard::<VnSystemUiProfileManifest>("vn.system_ui_profile_manifest")
    {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.system_ui_profile".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.system_ui_profile_manifest section is missing or invalid".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_SYSTEM_UI_PROFILE_MANIFEST",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.system_ui_profile_manifest"),
                    evidence("profile", profile),
                ],
            };
        }
    };
    let profile_report = profile_manifest.validate();
    if profile_report.status == SystemStoryValidationStatus::Blocked {
        return ReleaseCheckRecord {
            id: "vn.system_ui_profile".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "VN system UI profile is missing migration, unlock or localization coverage"
                .to_string(),
            diagnostic: profile_report.diagnostics.first().cloned(),
            evidence: vec![
                evidence("section", "vn.system_ui_profile_manifest"),
                evidence("profile", profile),
                evidence("diagnostic_count", profile_report.diagnostics.len()),
            ],
        };
    }

    ReleaseCheckRecord {
        id: "vn.system_ui_profile".to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "VN system UI profile declares required stories and coverage policies".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", "vn.system_story_manifest"),
            evidence("profile_section", "vn.system_ui_profile_manifest"),
            evidence("profile", profile),
            evidence("page_count", manifest.entries.len()),
            evidence("required_count", required.len()),
            evidence("unlock_source_count", profile_manifest.unlock_sources.len()),
            evidence(
                "localization_locale_count",
                profile_manifest.localization.locales.len(),
            ),
            evidence("save_schema", profile_manifest.save_schema),
        ],
    }
}

pub(crate) fn vn_advanced_presentation_check(
    package: &PackageReader,
    profile: &str,
) -> ReleaseCheckRecord {
    let manifest = match package
        .container()
        .decode_postcard::<VnAdvancedPresentationManifest>("vn.advanced_presentation_manifest")
    {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.advanced_presentation".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.advanced_presentation_manifest section is missing or invalid"
                    .to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_ADVANCED_PRESENTATION_MANIFEST",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.advanced_presentation_manifest"),
                    evidence("profile", profile),
                ],
            };
        }
    };

    let report = manifest.validate_required();
    if !report.passed {
        return ReleaseCheckRecord {
            id: "vn.advanced_presentation".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "VN advanced presentation profile is missing automated evidence".to_string(),
            diagnostic: report.diagnostics.first().cloned(),
            evidence: vec![
                evidence("section", "vn.advanced_presentation_manifest"),
                evidence("profile", profile),
                evidence("story_hash", manifest.story_hash),
                evidence("required_count", report.required_count),
                evidence("evidence_count", report.evidence_count),
                evidence("diagnostic_count", report.diagnostics.len()),
            ],
        };
    }

    ReleaseCheckRecord {
        id: "vn.advanced_presentation".to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "VN advanced presentation profile declares required automated evidence"
            .to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", "vn.advanced_presentation_manifest"),
            evidence("profile", profile),
            evidence("story_hash", manifest.story_hash),
            evidence("required_count", report.required_count),
            evidence("evidence_count", report.evidence_count),
            evidence("timeline_count", manifest.timeline_ids.len()),
        ],
    }
}
