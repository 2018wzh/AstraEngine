use super::*;

pub(crate) fn tsuinosora_checks(
    package: &PackageReader,
    profile: &str,
    selected_target: Option<&str>,
) -> Vec<ReleaseCheckRecord> {
    let Some(target) = tsuinosora_target(package, selected_target) else {
        return Vec::new();
    };
    let mut checks = vec![
        tsuinosora_reference_evidence_check(package),
        tsuinosora_asset_analysis_check(package),
        tsuinosora_conversion_manifest_check(package),
        tsuinosora_mount_policy_check(package, &target),
    ];
    if profile == "modern" {
        checks.push(tsuinosora_modern_profile_check(package));
    }
    checks
}

pub(crate) fn tsuinosora_target(
    package: &PackageReader,
    selected_target: Option<&str>,
) -> Option<String> {
    if let Some(target) = selected_target {
        if target.starts_with("tsuinosora-") {
            return Some(target.to_string());
        }
    }
    let bytes = package
        .container()
        .read_bounded("target.manifest", 256 * 1024)
        .ok()?;
    let manifest: TargetManifest = serde_json::from_slice(&bytes).ok()?;
    manifest
        .targets
        .iter()
        .find(|target| target.id.starts_with("tsuinosora-"))
        .map(|target| target.id.clone())
}

pub(crate) fn tsuinosora_reference_evidence_check(package: &PackageReader) -> ReleaseCheckRecord {
    let section = "tsuinosora.reference_evidence";
    let value = match read_tsuinosora_json_section(package, section) {
        Ok(value) => value,
        Err(check) => return *check,
    };
    if let Some(check) = tsuinosora_schema_check(
        section,
        &value,
        &[
            "tsuinosora.visual_reference_report.v1",
            "tsuinosora.reference_evidence.v1",
        ],
    ) {
        return check;
    }
    if let Some(check) = tsuinosora_path_leak_check(section, &value) {
        return check;
    }
    if let Some(check) = tsuinosora_status_check(section, &value) {
        return check;
    }
    let references = value
        .get("references")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if references.is_empty() {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_REFERENCE_EVIDENCE",
            "TsuiNoSora reference evidence must include title/game reference hashes",
            vec![evidence("section", section), evidence("reference_count", 0)],
        );
    }
    let missing_hash = references.iter().any(|reference| {
        reference
            .get("hash")
            .and_then(serde_json::Value::as_str)
            .is_none()
    });
    if missing_hash {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_REFERENCE_HASH",
            "TsuiNoSora reference evidence entries must expose hashes",
            vec![evidence("section", section)],
        );
    }
    for (logical_id, expected_hash, (expected_width, expected_height)) in [
        (
            "title",
            TSUI_REFERENCE_TITLE_HASH,
            TSUI_REFERENCE_TITLE_DIMENSIONS,
        ),
        (
            "game",
            TSUI_REFERENCE_GAME_HASH,
            TSUI_REFERENCE_GAME_DIMENSIONS,
        ),
    ] {
        let Some(reference) = references.iter().find(|reference| {
            reference
                .get("logical_id")
                .and_then(serde_json::Value::as_str)
                == Some(logical_id)
        }) else {
            return tsuinosora_blocked(
                section,
                "ASTRA_TSUI_REFERENCE_EVIDENCE",
                format!("TsuiNoSora reference evidence must include {logical_id}"),
                vec![
                    evidence("section", section),
                    evidence("logical_id", logical_id),
                ],
            );
        };
        let actual_hash = reference
            .get("hash")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if actual_hash != expected_hash {
            return tsuinosora_blocked(
                section,
                "ASTRA_TSUI_REFERENCE_HASH_MISMATCH",
                format!("TsuiNoSora {logical_id} reference hash does not match authority"),
                vec![
                    evidence("section", section),
                    evidence("logical_id", logical_id),
                    evidence("expected_hash", expected_hash),
                    evidence("actual_hash", actual_hash),
                ],
            );
        }
        let width = reference
            .get("dimensions")
            .and_then(|dimensions| dimensions.get("width"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or_default();
        let height = reference
            .get("dimensions")
            .and_then(|dimensions| dimensions.get("height"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or_default();
        if width != expected_width || height != expected_height {
            return tsuinosora_blocked(
                section,
                "ASTRA_TSUI_REFERENCE_DIMENSION_MISMATCH",
                format!("TsuiNoSora {logical_id} reference dimensions do not match authority"),
                vec![
                    evidence("section", section),
                    evidence("logical_id", logical_id),
                    evidence("expected_width", expected_width),
                    evidence("expected_height", expected_height),
                    evidence("actual_width", width),
                    evidence("actual_height", height),
                ],
            );
        }
    }
    ReleaseCheckRecord {
        id: section.to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "TsuiNoSora visual reference evidence is present and sanitized".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", section),
            evidence("reference_count", references.len()),
        ],
    }
}

pub(crate) fn tsuinosora_asset_analysis_check(package: &PackageReader) -> ReleaseCheckRecord {
    let section = "tsuinosora.asset_analysis";
    let value = match read_tsuinosora_json_section(package, section) {
        Ok(value) => value,
        Err(check) => return *check,
    };
    if let Some(check) = tsuinosora_schema_check(section, &value, &["tsuinosora.asset_analysis.v1"])
    {
        return check;
    }
    if let Some(check) = tsuinosora_path_leak_check(section, &value) {
        return check;
    }
    let quarantine_count = value
        .get("quarantine")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if quarantine_count > 0 {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_ASSET_QUARANTINE",
            "TsuiNoSora asset analysis contains quarantined assets",
            vec![
                evidence("section", section),
                evidence("quarantine_count", quarantine_count),
            ],
        );
    }
    if let Some(check) = tsuinosora_status_check(section, &value) {
        return check;
    }
    let asset_count = value
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if asset_count == 0 {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_ASSET_ANALYSIS_EMPTY",
            "TsuiNoSora asset analysis must include analyzed asset evidence",
            vec![evidence("section", section), evidence("asset_count", 0)],
        );
    }
    ReleaseCheckRecord {
        id: section.to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "TsuiNoSora asset analysis passed with no quarantine".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", section),
            evidence("asset_count", asset_count),
            evidence("quarantine_count", quarantine_count),
        ],
    }
}

pub(crate) fn tsuinosora_conversion_manifest_check(package: &PackageReader) -> ReleaseCheckRecord {
    let section = "tsuinosora.conversion_manifest";
    let value = match read_tsuinosora_json_section(package, section) {
        Ok(value) => value,
        Err(check) => return *check,
    };
    if let Some(check) = tsuinosora_schema_check(
        section,
        &value,
        &[
            "tsuinosora.conversion_report.v1",
            "tsuinosora.conversion_manifest.v1",
        ],
    ) {
        return check;
    }
    if let Some(check) = tsuinosora_path_leak_check(section, &value) {
        return check;
    }
    if let Some(check) = tsuinosora_status_check(section, &value) {
        return check;
    }
    let routes = value
        .get("routes")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if routes.is_empty() {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_ROUTE_COVERAGE",
            "TsuiNoSora conversion manifest must include covered routes",
            vec![evidence("section", section), evidence("route_count", 0)],
        );
    }
    let uncovered = routes.iter().filter(|route| {
        route.get("coverage").and_then(serde_json::Value::as_str) != Some("covered")
    });
    let uncovered_count = uncovered.count();
    if uncovered_count > 0 {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_ROUTE_COVERAGE",
            "TsuiNoSora conversion manifest contains routes without proven coverage",
            vec![
                evidence("section", section),
                evidence("route_count", routes.len()),
                evidence("uncovered_count", uncovered_count),
            ],
        );
    }
    let resources = value
        .get("resources")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if resources.is_empty() {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_CONVERSION_RESOURCE_EVIDENCE",
            "TsuiNoSora conversion manifest must include converted resource evidence",
            vec![evidence("section", section), evidence("resource_count", 0)],
        );
    }
    if let Some(field) = tsuinosora_invalid_conversion_resource_field(resources) {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_CONVERSION_RESOURCE_EVIDENCE",
            "TsuiNoSora conversion manifest resources must include path, hash and byte size evidence",
            vec![
                evidence("section", section),
                evidence("resource_count", resources.len()),
                evidence("field", field),
            ],
        );
    }
    ReleaseCheckRecord {
        id: section.to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "TsuiNoSora conversion manifest proves route and resource coverage".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", section),
            evidence("route_count", routes.len()),
            evidence("resource_count", resources.len()),
        ],
    }
}

pub(crate) fn tsuinosora_invalid_conversion_resource_field(
    resources: &[serde_json::Value],
) -> Option<String> {
    for (index, resource) in resources.iter().enumerate() {
        for field in ["source", "native_path", "classification"] {
            if resource
                .get(field)
                .and_then(serde_json::Value::as_str)
                .is_none_or(str::is_empty)
            {
                return Some(format!("resources.{index}.{field}"));
            }
        }
        for field in ["source_hash", "converted_hash"] {
            let value = resource
                .get(field)
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if !is_release_sha256(value) {
                return Some(format!("resources.{index}.{field}"));
            }
        }
        if resource
            .get("byte_size")
            .and_then(serde_json::Value::as_u64)
            .is_none_or(|byte_size| byte_size == 0)
        {
            return Some(format!("resources.{index}.byte_size"));
        }
    }
    None
}

pub(crate) fn is_release_sha256(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn tsuinosora_mount_policy_check(
    package: &PackageReader,
    target: &str,
) -> ReleaseCheckRecord {
    let section = "tsuinosora.mount_policy";
    let value = match read_tsuinosora_json_section(package, section) {
        Ok(value) => value,
        Err(check) => return *check,
    };
    if let Some(check) = tsuinosora_schema_check(section, &value, &["tsuinosora.mount_policy.v1"]) {
        return check;
    }
    if let Some(check) = tsuinosora_path_leak_check(section, &value) {
        return check;
    }
    if let Some(check) = tsuinosora_status_check(section, &value) {
        return check;
    }
    if value
        .get("target")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|manifest_target| manifest_target != target)
    {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_MOUNT_TARGET",
            "TsuiNoSora mount policy target must match the selected target",
            vec![
                evidence("section", section),
                evidence("selected_target", target),
            ],
        );
    }
    let aliases = value
        .get("aliases")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if aliases.is_empty() {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_MOUNT_ALIAS",
            "TsuiNoSora mount policy must declare mount aliases",
            vec![evidence("section", section)],
        );
    }
    ReleaseCheckRecord {
        id: section.to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "TsuiNoSora mount policy is sanitized and target-bound".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", section),
            evidence("target", target),
            evidence("alias_count", aliases.len()),
        ],
    }
}
