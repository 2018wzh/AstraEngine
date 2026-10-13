use super::*;

pub(crate) fn package_target_shape_diagnostic(manifest: &TargetManifest) -> Option<Diagnostic> {
    if manifest.targets.len() != 1 {
        return Some(Diagnostic::blocking(
            "ASTRA_TARGET_PACKAGE_SHAPE",
            "package target manifest must contain exactly one target",
        ));
    }
    let target = &manifest.targets[0];
    if target.kind != TargetKind::Game || !target.packaged {
        return Some(
            Diagnostic::blocking(
                "ASTRA_TARGET_PACKAGE_GAME",
                "package target manifest must contain one packaged game target",
            )
            .with_field("target", &target.id),
        );
    }
    None
}

pub(crate) fn media_check(require_ffmpeg: bool) -> ReleaseCheckRecord {
    let symphonia = astra_media::SymphoniaAudioDecodeProvider;
    let symphonia_available = symphonia.probe_available();
    if require_ffmpeg {
        match astra_media::probe_ffmpeg_provider() {
            Ok(capability) => ReleaseCheckRecord {
                id: "media.decode.ffmpeg".to_string(),
                domain: ReleaseDomain::Media,
                status: CheckStatus::Pass,
                summary: "required FFmpeg provider was compiled and passed its native probe"
                    .to_string(),
                diagnostic: None,
                evidence: vec![
                    evidence("provider_id", capability.provider_id),
                    evidence("codecs", capability.codecs.join(",")),
                    evidence("packaged_eligible", capability.packaged_eligible),
                    evidence("symphonia_available", symphonia_available),
                ],
            },
            Err(error) => ReleaseCheckRecord {
                id: "media.decode.ffmpeg".to_string(),
                domain: ReleaseDomain::Media,
                status: CheckStatus::Blocked,
                summary: "required FFmpeg provider is unavailable".to_string(),
                diagnostic: Some(ffmpeg_probe_diagnostic(error)),
                evidence: vec![
                    evidence("ffmpeg_compiled", astra_media::ffmpeg_compiled()),
                    evidence("symphonia_available", symphonia_available),
                ],
            },
        }
    } else {
        ReleaseCheckRecord {
            id: "media.decode.platform_fallback".to_string(),
            domain: ReleaseDomain::Media,
            status: CheckStatus::Warning,
            summary:
                "FFmpeg fallback is optional for this profile; platform decode remains preferred"
                    .to_string(),
            diagnostic: Some(Diagnostic::warning(
                "ASTRA_MEDIA_FFMPEG_OPTIONAL",
                "FFmpeg feature is not required for this validation profile",
            )),
            evidence: vec![
                evidence("ffmpeg_compiled", astra_media::ffmpeg_compiled()),
                evidence("symphonia_available", symphonia_available),
            ],
        }
    }
}

pub(crate) fn ffmpeg_probe_diagnostic(error: astra_media::MediaError) -> Diagnostic {
    match error {
        astra_media::MediaError::Diagnostics(diagnostics) => {
            diagnostics.into_iter().next().unwrap_or_else(|| {
                Diagnostic::blocking(
                    "ASTRA_MEDIA_FFMPEG_PROBE",
                    "FFmpeg probe failed without a provider diagnostic",
                )
            })
        }
        astra_media::MediaError::Message(message) => {
            Diagnostic::blocking("ASTRA_MEDIA_FFMPEG_PROBE", message)
        }
    }
}

pub(crate) fn platform_report_check(
    report: Option<&PlatformCapabilityReport>,
    require_report: bool,
) -> ReleaseCheckRecord {
    let Some(report) = report else {
        return ReleaseCheckRecord {
            id: "platform.capability_report".to_string(),
            domain: ReleaseDomain::Platform,
            status: if require_report {
                CheckStatus::Blocked
            } else {
                CheckStatus::Warning
            },
            summary: "platform capability report was not supplied".to_string(),
            diagnostic: Some(if require_report {
                Diagnostic::blocking(
                    "ASTRA_PLATFORM_REPORT_MISSING",
                    "platform probe evidence is required for this release profile",
                )
            } else {
                Diagnostic::warning(
                    "ASTRA_PLATFORM_REPORT_MISSING",
                    "platform probe evidence is required before native platform completion",
                )
            }),
            evidence: vec![],
        };
    };

    let (status, diagnostics) = astra_platform::validate_capability_report(report);
    let mut evidence_values = vec![
        evidence("platform", report.platform),
        evidence("target", &report.target),
        evidence("profile_id", &report.profile_id),
        evidence("profile_hash", &report.profile_hash),
        evidence("build_fingerprint", &report.build_fingerprint),
        evidence(
            "sdk_status",
            format!("{:?}", report.sdk_status).to_lowercase(),
        ),
    ];
    for (domain, selection) in [
        ("renderer", &report.renderer),
        ("decode", &report.decode),
        ("audio_mixer", &report.audio_mixer),
        ("audio_output", &report.audio_output),
        ("save", &report.save),
    ] {
        evidence_values.push(evidence(
            format!("provider.{domain}.declared"),
            selection.declared.join(","),
        ));
        evidence_values.push(evidence(
            format!("provider.{domain}.available"),
            selection.available.join(","),
        ));
        evidence_values.push(evidence(
            format!("provider.{domain}.selected"),
            selection.selected.as_deref().unwrap_or_default(),
        ));
    }

    ReleaseCheckRecord {
        id: "platform.capability_report".to_string(),
        domain: ReleaseDomain::Platform,
        status: match status {
            PlatformValidationStatus::Pass => CheckStatus::Pass,
            PlatformValidationStatus::Warning => CheckStatus::Warning,
            PlatformValidationStatus::Blocked => CheckStatus::Blocked,
        },
        summary: "platform capability report binds declared, available and selected providers"
            .to_string(),
        diagnostic: diagnostics.first().cloned(),
        evidence: evidence_values,
    }
}

pub(crate) fn platform_profile_binding_check(
    package: &PackageReader,
    capability: Option<&PlatformCapabilityReport>,
    required: bool,
) -> ReleaseCheckRecord {
    let blocked = |code: &'static str, summary: &'static str| ReleaseCheckRecord {
        id: "platform.profile_binding".to_string(),
        domain: ReleaseDomain::Platform,
        status: if required {
            CheckStatus::Blocked
        } else {
            CheckStatus::Warning
        },
        summary: summary.to_string(),
        diagnostic: Some(if required {
            Diagnostic::blocking(code, summary)
        } else {
            Diagnostic::warning(code, summary)
        }),
        evidence: Vec::new(),
    };
    if !package.has_section("platform.profiles") {
        return blocked(
            "ASTRA_PLATFORM_PROFILE_SECTION_MISSING",
            "package does not contain cooked platform profiles",
        );
    }
    let Some(capability) = capability else {
        return blocked(
            "ASTRA_PLATFORM_PROFILE_CAPABILITY_MISSING",
            "platform profile binding requires a capability report",
        );
    };
    let value = match read_json_section(package, "platform.profiles") {
        Ok(value) => value,
        Err(_) => {
            return blocked(
                "ASTRA_PLATFORM_PROFILE_SECTION_INVALID",
                "cooked platform profile section is invalid",
            )
        }
    };
    if !matches!(
        value.get("schema").and_then(serde_json::Value::as_str),
        Some("astra.platform_profiles.v3")
    ) {
        return blocked(
            "ASTRA_PLATFORM_PROFILE_SECTION_INVALID",
            "cooked platform profile section schema is unsupported",
        );
    }
    let profiles: Vec<astra_platform::PlatformHostProfile> = match value
        .get("profiles")
        .cloned()
        .and_then(|profiles| serde_json::from_value::<Vec<serde_json::Value>>(profiles).ok())
        .and_then(|profiles| {
            profiles
                .into_iter()
                .map(astra_platform::migrate_host_profile_json)
                .collect::<Result<Vec<_>, _>>()
                .ok()
        }) {
        Some(profiles) => profiles,
        None => {
            return blocked(
                "ASTRA_PLATFORM_PROFILE_SECTION_INVALID",
                "cooked platform profile list is invalid",
            )
        }
    };
    let matched = profiles.iter().find(|profile| {
        profile.platform == capability.platform
            && profile.target == capability.target
            && profile.id == capability.profile_id
            && profile.hash().ok().as_deref() == Some(capability.profile_hash.as_str())
    });
    let Some(profile) = matched else {
        return blocked(
            "ASTRA_PLATFORM_PROFILE_IDENTITY",
            "capability report does not match a cooked platform profile",
        );
    };
    if let Err(error) = astra_platform::validate_host_profile(profile) {
        return ReleaseCheckRecord {
            id: "platform.profile_binding".to_string(),
            domain: ReleaseDomain::Platform,
            status: CheckStatus::Blocked,
            summary: "cooked platform profile violates release policy".to_string(),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_PLATFORM_PROFILE_POLICY",
                error.to_string(),
            )),
            evidence: Vec::new(),
        };
    }
    ReleaseCheckRecord {
        id: "platform.profile_binding".to_string(),
        domain: ReleaseDomain::Platform,
        status: CheckStatus::Pass,
        summary: "capability report matches the cooked platform profile".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("profile_id", &capability.profile_id),
            evidence("profile_hash", &capability.profile_hash),
        ],
    }
}

pub(crate) fn evidence(key: impl Into<String>, value: impl ToString) -> ReleaseEvidence {
    ReleaseEvidence {
        key: key.into(),
        value: value.to_string(),
    }
}
