use super::*;

pub(crate) fn tsuinosora_modern_profile_check(package: &PackageReader) -> ReleaseCheckRecord {
    let section = "tsuinosora.modern_profile_report";
    let value = match read_tsuinosora_json_section(package, section) {
        Ok(value) => value,
        Err(check) => return *check,
    };
    if let Some(check) =
        tsuinosora_schema_check(section, &value, &["tsuinosora.modern_profile_report.v1"])
    {
        return check;
    }
    if let Some(check) = tsuinosora_path_leak_check(section, &value) {
        return check;
    }
    if let Some(check) = tsuinosora_status_check(section, &value) {
        return check;
    }
    let features = value
        .get("features")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if features.is_empty()
        || features.iter().any(|feature| {
            !feature
                .get("independent_switch")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
                || feature
                    .get("affects_core_state")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true)
                || feature
                    .get("fallback_hash")
                    .and_then(serde_json::Value::as_str)
                    .is_none_or(str::is_empty)
        })
    {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_MODERN_FEATURE",
            "TsuiNoSora modern profile features must be reversible, independent and backed by fallback hashes",
            vec![
                evidence("section", section),
                evidence("feature_count", features.len()),
            ],
        );
    }
    ReleaseCheckRecord {
        id: section.to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "TsuiNoSora modern profile report is reversible and fallback-backed".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", section),
            evidence("feature_count", features.len()),
        ],
    }
}

pub(crate) fn read_tsuinosora_json_section(
    package: &PackageReader,
    section: &str,
) -> Result<serde_json::Value, Box<ReleaseCheckRecord>> {
    let bytes = match package.container().read_bounded(section, 512 * 1024) {
        Ok(bytes) => bytes,
        Err(err) => {
            return Err(Box::new(tsuinosora_blocked(
                section,
                "ASTRA_TSUI_SECTION_MISSING",
                format!("missing or unreadable TsuiNoSora package section {section}: {err}"),
                vec![evidence("section", section)],
            )));
        }
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|err| {
        Box::new(tsuinosora_blocked(
            section,
            "ASTRA_TSUI_SECTION_JSON",
            format!("TsuiNoSora package section {section} is not valid JSON: {err}"),
            vec![evidence("section", section)],
        ))
    })?;
    if let Some(check) = tsuinosora_payload_leak_check(section, &value) {
        return Err(Box::new(check));
    }
    Ok(value)
}

pub(crate) fn tsuinosora_schema_check(
    section: &str,
    value: &serde_json::Value,
    expected: &[&str],
) -> Option<ReleaseCheckRecord> {
    let schema = value.get("schema").and_then(serde_json::Value::as_str);
    if schema.is_none_or(|schema| !expected.contains(&schema)) {
        return Some(tsuinosora_blocked(
            section,
            "ASTRA_TSUI_SCHEMA",
            "TsuiNoSora package section uses an unexpected schema",
            vec![
                evidence("section", section),
                evidence("schema", schema.unwrap_or("missing")),
                evidence("expected", expected.join("|")),
            ],
        ));
    }
    None
}

pub(crate) fn tsuinosora_status_check(
    section: &str,
    value: &serde_json::Value,
) -> Option<ReleaseCheckRecord> {
    let status = value.get("status").and_then(serde_json::Value::as_str);
    if status != Some("pass") {
        return Some(tsuinosora_blocked(
            section,
            "ASTRA_TSUI_REPORT_BLOCKED",
            "TsuiNoSora report status is not pass",
            vec![
                evidence("section", section),
                evidence("status", status.unwrap_or("missing")),
            ],
        ));
    }
    None
}

pub(crate) fn tsuinosora_path_leak_check(
    section: &str,
    value: &serde_json::Value,
) -> Option<ReleaseCheckRecord> {
    if json_has_local_path(value) {
        return Some(tsuinosora_blocked(
            section,
            "ASTRA_TSUI_REPORT_PATH_LEAK",
            "TsuiNoSora package report contains a local path-like value",
            vec![evidence("section", section)],
        ));
    }
    None
}

pub(crate) fn tsuinosora_payload_leak_check(
    section: &str,
    value: &serde_json::Value,
) -> Option<ReleaseCheckRecord> {
    let field = forbidden_tsuinosora_payload_field(value)?;
    Some(tsuinosora_blocked(
        section,
        "ASTRA_TSUI_REPORT_PAYLOAD_LEAK",
        "TsuiNoSora package report contains a commercial payload-like field",
        vec![evidence("section", section), evidence("field", field)],
    ))
}

pub(crate) fn forbidden_tsuinosora_payload_field(value: &serde_json::Value) -> Option<String> {
    fn visit(value: &serde_json::Value, path: &mut Vec<String>) -> Option<String> {
        match value {
            serde_json::Value::Array(values) => {
                for (index, child) in values.iter().enumerate() {
                    path.push(index.to_string());
                    if let Some(field) = visit(child, path) {
                        return Some(field);
                    }
                    path.pop();
                }
                None
            }
            serde_json::Value::Object(values) => {
                for (key, child) in values {
                    path.push(key.clone());
                    if is_forbidden_tsuinosora_payload_key(key, path, child) {
                        return Some(path.join("."));
                    }
                    if let Some(field) = visit(child, path) {
                        return Some(field);
                    }
                    path.pop();
                }
                None
            }
            _ => None,
        }
    }

    visit(value, &mut Vec::new())
}

pub(crate) fn is_forbidden_tsuinosora_payload_key(
    key: &str,
    path: &[String],
    value: &serde_json::Value,
) -> bool {
    let key = key.to_ascii_lowercase();
    if key == "payload" {
        return !(path == ["redaction".to_string(), "payload".to_string()]
            && value.as_str() == Some("omitted"));
    }
    matches!(
        key.as_str(),
        "body"
            | "bytecode"
            | "bytes"
            | "commercial_text"
            | "content"
            | "lingo_source"
            | "payload_bytes"
            | "raw_payload"
            | "script_text"
            | "source_payload"
            | "source_text"
            | "text"
    )
}

pub(crate) fn json_has_local_path(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(value) => looks_like_local_path(value),
        serde_json::Value::Array(values) => values.iter().any(json_has_local_path),
        serde_json::Value::Object(values) => values.values().any(json_has_local_path),
        _ => false,
    }
}

pub(crate) fn looks_like_local_path(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with("\\\\")
        || value.as_bytes().windows(3).any(|pair| {
            pair[0].is_ascii_alphabetic() && pair[1] == b':' && matches!(pair[2], b'/' | b'\\')
        })
}

pub(crate) fn tsuinosora_blocked(
    id: impl Into<String>,
    code: &'static str,
    summary: impl Into<String>,
    evidence_values: Vec<ReleaseEvidence>,
) -> ReleaseCheckRecord {
    let summary = summary.into();
    ReleaseCheckRecord {
        id: id.into(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Blocked,
        summary: summary.clone(),
        diagnostic: Some(Diagnostic::blocking(code, summary)),
        evidence: evidence_values,
    }
}
