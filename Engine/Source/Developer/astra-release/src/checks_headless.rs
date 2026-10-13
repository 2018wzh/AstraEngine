use super::*;

pub(crate) fn engine_test_profile_isolation_check(
    package: &PackageReader,
    profile: &str,
) -> ReleaseCheckRecord {
    let has_nativevn_profile_manifest = package
        .container()
        .decode_postcard::<VnProfileManifest>("vn.profile_manifest")
        .is_ok_and(|manifest| manifest.profiles.iter().any(|entry| entry == "minimal"));
    let has_nativevn_target = package
        .container()
        .read_bounded("target.manifest", 256 * 1024)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<TargetManifest>(&bytes).ok())
        .is_some_and(|manifest| {
            manifest
                .targets
                .iter()
                .any(|target| target.kind == TargetKind::Game && target.packaged)
        });
    let is_nativevn_engine_test_profile =
        profile == "minimal" && (has_nativevn_profile_manifest || has_nativevn_target);
    if is_nativevn_engine_test_profile {
        return ReleaseCheckRecord {
            id: "profile.engine_test_isolation".to_string(),
            domain: ReleaseDomain::Target,
            status: CheckStatus::Blocked,
            summary: "Engine-only NativeVN profile cannot be released".to_string(),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_ENGINE_TEST_PROFILE_RELEASE",
                "NativeVN minimal profile is reserved for Engine tests",
            )),
            evidence: vec![evidence("profile", profile)],
        };
    }
    ReleaseCheckRecord {
        id: "profile.engine_test_isolation".to_string(),
        domain: ReleaseDomain::Target,
        status: CheckStatus::Pass,
        summary: "release profile is not reserved for Engine tests".to_string(),
        diagnostic: None,
        evidence: vec![evidence("profile", profile)],
    }
}

pub(crate) fn headless_shipping_isolation_check(package: &PackageReader) -> ReleaseCheckRecord {
    // Retired test formats remain forbidden in shipping payloads, not readable.
    const FORBIDDEN: [&str; 11] = [
        "astra.headless_host_profile.v3",
        "astra.headless_protocol.v1",
        "astra.headless_checkpoint_config.v2",
        "astra.headless_checkpoint_config.v3",
        "astra.headless_tolerance_approval.v2",
        "astra.user_input_sequence.v1",
        "astra.headless_artifact_manifest.v2",
        "astra.headless_run_report.v2",
        "astra.headless_review.v2",
        "astra.headless_preflight_link.v2",
        "astra-platform-headless",
    ];
    let mut violation = None;
    for entry in package.container().entries() {
        if entry.id.to_ascii_lowercase().contains("headless") {
            violation = Some(format!("section id {} is test-only", entry.id));
            break;
        }
        if entry.encryption.is_none() && entry.decoded_length <= 1024 * 1024 {
            if let Ok(bytes) = package.container().read_section(&entry.id) {
                if FORBIDDEN.iter().any(|needle| {
                    bytes
                        .windows(needle.len())
                        .any(|window| window == needle.as_bytes())
                }) {
                    violation = Some(format!(
                        "section {} contains a test-only Headless identity",
                        entry.id
                    ));
                    break;
                }
            }
        }
    }
    let passed = violation.is_none();
    ReleaseCheckRecord {
        id: "shipping.headless_isolation".to_string(),
        domain: ReleaseDomain::Platform,
        status: if passed {
            CheckStatus::Pass
        } else {
            CheckStatus::Blocked
        },
        summary: if passed {
            "shipping package excludes Headless test schemas and providers".to_string()
        } else {
            violation.unwrap_or_else(|| "Headless isolation failed".to_string())
        },
        diagnostic: (!passed).then(|| {
            Diagnostic::blocking(
                "ASTRA_RELEASE_HEADLESS_FORBIDDEN",
                "Headless test identities cannot enter a shipping package",
            )
        }),
        evidence: vec![evidence("forbidden_identity_count", FORBIDDEN.len())],
    }
}
