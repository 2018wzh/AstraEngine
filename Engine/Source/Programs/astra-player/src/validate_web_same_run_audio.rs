use super::*;

pub(super) fn validate_web_same_run_audio(
    evidence: &WebCdpRuntimeEvidence,
    meter: &PlayerAudioMeterEvidence,
    identity: &PlayerPlatformEvidenceIdentity,
) -> Result<(), PlayerAutomationError> {
    if evidence.session_id.as_deref() != Some(identity.session_id.as_str()) {
        return Err("ASTRA_PLAYER_WEB_SESSION_IDENTITY_MISMATCH".into());
    }
    let value = evidence
        .audio_meter
        .as_ref()
        .ok_or("ASTRA_PLAYER_WEB_AUDIO_EVIDENCE_MISSING")?;
    let u64_field = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| -> PlayerAutomationError {
                format!("ASTRA_PLAYER_WEB_AUDIO_FIELD_MISSING: {key}").into()
            })
    };
    let callback_count = u64_field("callback_count")?;
    let consumed_samples = u64_field("consumed_samples")?;
    let peak_bits = u64_field("peak_dbfs_bits")?;
    let rms_bits = u64_field("rms_dbfs_bits")?;
    let peak_bits = u32::try_from(peak_bits).map_err(|_| "ASTRA_PLAYER_WEB_AUDIO_PEAK_RANGE")?;
    let rms_bits = u32::try_from(rms_bits).map_err(|_| "ASTRA_PLAYER_WEB_AUDIO_RMS_RANGE")?;
    if callback_count != meter.callback_count
        || consumed_samples != meter.sample_count
        || f32::from_bits(peak_bits).to_bits() != meter.peak_dbfs.to_bits()
        || f32::from_bits(rms_bits).to_bits() != meter.rms_dbfs.to_bits()
    {
        return Err("ASTRA_PLAYER_WEB_AUDIO_REPORT_MISMATCH".into());
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(super) struct BundleContext {
    pub(super) bundle_dir: PathBuf,
    #[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
    pub(super) entrypoint: String,
    pub(super) target: String,
    pub(super) profile: String,
    pub(super) platform: String,
    pub(super) package_hash: String,
    pub(super) package_path: PathBuf,
    pub(super) scenario_refs: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct BundleAutomationScenario {
    pub(super) scenario_ref: String,
    pub(super) route_id: String,
    pub(super) inputs: Vec<ScenarioInputAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ScenarioHostInput {
    pub(super) kind: &'static str,
    pub(super) virtual_key: u16,
}

impl BundleContext {
    pub(super) fn read(bundle_dir: PathBuf) -> Result<Self, PlayerAutomationError> {
        let manifest_path = bundle_dir.join("bundle_manifest.json");
        let manifest: serde_json::Value = serde_json::from_slice(&fs::read(manifest_path)?)?;
        let entrypoint = required_string(&manifest, "entrypoint")?;
        let target = required_string(&manifest, "target")?;
        let profile = required_string(&manifest, "profile")?;
        let platform = required_string(&manifest, "platform")?;
        let package_rel = required_string(&manifest, "package")?;
        if !is_safe_relative_ref(&entrypoint) || !is_safe_relative_ref(&package_rel) {
            return Err("bundle manifest contains an unsafe relative path".into());
        }
        let scenario_refs = manifest
            .get("scenario_refs")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .filter(|value| is_safe_relative_ref(value))
            .map(str::to_string)
            .collect::<Vec<_>>();
        Ok(Self {
            bundle_dir: bundle_dir.clone(),
            #[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
            entrypoint,
            target,
            profile,
            platform,
            package_hash: required_string(&manifest, "package_hash")?,
            package_path: bundle_dir.join(package_rel),
            scenario_refs,
        })
    }

    pub(super) fn automation_scenario(
        &self,
    ) -> Result<BundleAutomationScenario, PlayerAutomationError> {
        let refs = self
            .scenario_refs
            .iter()
            .filter(|scenario_ref| {
                let platform_marker = format!(".{}.", self.platform);
                scenario_ref.contains(&platform_marker) || self.scenario_refs.len() == 1
            })
            .collect::<Vec<_>>();
        if refs.len() != 1 {
            return Err(format!(
                "ASTRA_PLAYER_AUTOMATION_SCENARIO_COUNT: expected exactly one {} scenario, found {}",
                self.platform,
                refs.len()
            )
            .into());
        }
        let scenario_ref = refs[0];
        let bytes = fs::read(self.bundle_dir.join(scenario_ref))?;
        let value: serde_yaml::Value = serde_yaml::from_slice(&bytes)?;
        let route_id = value
            .get("generated_route_id")
            .and_then(serde_yaml::Value::as_str)
            .ok_or("ASTRA_PLAYER_AUTOMATION_ROUTE_MISSING: scenario has no generated_route_id")?
            .to_string();
        Ok(BundleAutomationScenario {
            scenario_ref: scenario_ref.clone(),
            route_id,
            inputs: parse_scenario_input_plan(&bytes)
                .map_err(|error| -> PlayerAutomationError { error.into() })?,
        })
    }

    pub(super) fn resolve_host_inputs(
        &self,
        inputs: &[ScenarioInputAction],
    ) -> Result<Vec<ScenarioHostInput>, PlayerAutomationError> {
        let package_bytes = fs::read(&self.package_path)?;
        let package = astra_package::PackageReader::open(&package_bytes)?;
        let compiled = astra_vn_package::decode_compiled_project(&package)?;
        let mut option_indexes = std::collections::BTreeMap::new();
        for state in compiled.states.values() {
            for scene in &state.scenes {
                for command in &scene.commands {
                    if let astra_vn_core::CompiledCommand::Choice { options, .. } = command {
                        for (index, option) in options.iter().enumerate() {
                            if option_indexes.insert(option.id.clone(), index).is_some() {
                                return Err(format!(
                                    "ASTRA_PLAYER_CHOICE_ID_DUPLICATE: {}",
                                    option.id
                                )
                                .into());
                            }
                        }
                    }
                }
            }
        }
        inputs
            .iter()
            .map(|input| {
                let virtual_key = match input {
                    ScenarioInputAction::Advance => 0x20,
                    ScenarioInputAction::Choose { option_id } => {
                        let index = option_indexes.get(option_id).ok_or_else(|| {
                            format!("ASTRA_PLAYER_CHOICE_ID_UNKNOWN: {option_id}")
                        })?;
                        if *index >= 9 {
                            return Err(format!(
                                "ASTRA_PLAYER_CHOICE_KEY_UNAVAILABLE: {option_id} uses index {index}"
                            )
                            .into());
                        }
                        0x31 + *index as u16
                    }
                    ScenarioInputAction::OpenSystem { page } if page == "backlog" => 0x42,
                    ScenarioInputAction::OpenSystem { page } => {
                        return Err(format!(
                            "ASTRA_PLAYER_SYSTEM_KEY_UNAVAILABLE: no physical binding for {page}"
                        )
                        .into());
                    }
                    ScenarioInputAction::Back => 0x1B,
                };
                Ok(ScenarioHostInput {
                    kind: input.kind(),
                    virtual_key,
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub(super) struct LiveInputRun {
    pub(super) events: Vec<PlayerInputEvent>,
    pub(super) input_consumption: Vec<PlayerInputConsumptionEvidence>,
    pub(super) visual_regions: Vec<PlayerVisualRegionEvidence>,
    pub(super) runtime_routes: Vec<PlayerRuntimeRouteEvidence>,
    pub(super) route_coverage: Vec<String>,
}

pub(super) fn windows_live_input(
    bundle: &BundleContext,
    timeout_ms: u64,
    trace_log: Option<&PathBuf>,
    inputs: &[ScenarioHostInput],
) -> Result<LiveInputRun, PlayerAutomationError> {
    #[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
    {
        windows_live_input_impl(bundle, timeout_ms, trace_log, inputs)
    }
    #[cfg(not(all(target_os = "windows", feature = "platform-test-driver")))]
    {
        let _ = (bundle, timeout_ms, trace_log, inputs);
        Err("windows live automation requires Windows".into())
    }
}

pub(super) fn host_audio_meter(
    report_path: &PathBuf,
    package_hash: &str,
) -> Result<(PlayerAudioMeterEvidence, PlayerPlatformEvidenceIdentity), PlayerAutomationError> {
    let report: astra_platform::PlatformHostConformanceReport =
        serde_json::from_slice(&fs::read(report_path)?)?;
    if report.schema != astra_platform::PLATFORM_HOST_CONFORMANCE_REPORT_SCHEMA
        || report.status != astra_platform::ConformanceStatus::Pass
        || report.package_hash != package_hash
    {
        return Err("host conformance report does not match the bundle package".into());
    }
    let check = report
        .checks
        .iter()
        .find(|check| {
            check.id == "audio.output_meter"
                && check.status == astra_platform::ConformanceStatus::Pass
        })
        .ok_or("host conformance report is missing passing audio.output_meter evidence")?;
    let value = |key: &str| {
        check
            .evidence
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| entry.value.as_str())
            .ok_or_else(|| format!("audio.output_meter is missing {key} evidence"))
    };
    Ok((
        PlayerAudioMeterEvidence {
            provider: value("provider")?.to_string(),
            callback_count: value("callback_count")?.parse()?,
            host_report_hash: sha256_file(report_path)?,
            sample_count: value("sample_count")?.parse()?,
            peak_dbfs: value("peak_dbfs")?.parse()?,
            rms_dbfs: value("rms_dbfs")?.parse()?,
        },
        PlayerPlatformEvidenceIdentity {
            profile_hash: report.profile_hash,
            build_fingerprint: report.build_fingerprint,
            session_id: report.session_id,
        },
    ))
}

pub(super) fn visual_comparison_evidence(
    path: &PathBuf,
) -> Result<PlayerVisualComparisonEvidence, PlayerAutomationError> {
    let bytes = fs::read(path)?;
    let report: serde_json::Value = serde_json::from_slice(&bytes)?;
    let checkpoint_count = report
        .get("checkpoints")
        .and_then(serde_json::Value::as_array)
        .map(|items| items.len() as u32)
        .unwrap_or_default();
    let status = if report.get("schema").and_then(serde_json::Value::as_str)
        == Some("tsuinosora.visual_comparison_report.v1")
        && report.get("status").and_then(serde_json::Value::as_str) == Some("pass")
        && checkpoint_count > 0
    {
        PlayerAutomationStatus::Pass
    } else {
        PlayerAutomationStatus::Blocked
    };
    Ok(PlayerVisualComparisonEvidence {
        report_hash: Hash256::from_sha256(&bytes).to_string(),
        checkpoint_count,
        status,
    })
}

pub(super) fn required_string(
    value: &serde_json::Value,
    key: &str,
) -> Result<String, PlayerAutomationError> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("bundle manifest missing string field: {key}").into())
}

pub(super) fn sha256_file(path: &std::path::Path) -> Result<String, PlayerAutomationError> {
    Ok(Hash256::from_sha256_file(path)?.to_string())
}

pub(super) fn sha256_bytes(bytes: &[u8]) -> String {
    Hash256::from_sha256(bytes).to_string()
}

pub(super) fn is_safe_relative_ref(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.starts_with('\\')
        && !value.contains("://")
        && !value.contains('\\')
        && !value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.ends_with(':'))
}

#[cfg(any(test, all(target_os = "windows", feature = "platform-test-driver")))]
#[derive(Debug, Clone)]
pub(super) struct ParsedPlayerHostTrace {
    pub(super) player_sequence: u64,
    pub(super) fixed_step: u64,
    pub(super) kind: String,
    pub(super) trace_hash: String,
    pub(super) coverage_reached: Vec<String>,
    pub(super) session_id: String,
    pub(super) current_state_id: Option<String>,
    pub(super) pending_choice_ids: Vec<String>,
    pub(super) terminal_route_ids: Vec<String>,
}

#[cfg(any(test, all(target_os = "windows", feature = "platform-test-driver")))]
pub(super) fn parse_player_host_traces(stderr: &str) -> Vec<ParsedPlayerHostTrace> {
    let mut consumed = std::collections::BTreeMap::new();
    let mut runtime = std::collections::BTreeMap::new();
    for line in stderr.lines() {
        if line.contains("event=astra.player.input.consumed") {
            if let (Some(sequence), Some(kind)) = (
                trace_token(line, "player_sequence").and_then(|value| value.parse().ok()),
                trace_token(line, "kind"),
            ) {
                consumed.insert(sequence, (kind.to_string(), line));
            }
        } else if line.contains("event=astra.player.vn.step") {
            let Some(sequence) =
                trace_token(line, "player_sequence").and_then(|value| value.parse::<u64>().ok())
            else {
                continue;
            };
            let coverage = trace_token(line, "coverage")
                .filter(|value| *value != "-")
                .map(|value| {
                    value
                        .split(',')
                        .filter(|item| !item.is_empty())
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let Some(fixed_step) =
                trace_token(line, "fixed_step").and_then(|value| value.parse::<u64>().ok())
            else {
                continue;
            };
            let Some(session_id) = trace_token(line, "session_id").filter(|id| !id.is_empty())
            else {
                continue;
            };
            runtime.insert(
                sequence,
                (
                    coverage,
                    fixed_step,
                    session_id.to_string(),
                    trace_token(line, "current_state_id")
                        .filter(|value| *value != "-")
                        .map(str::to_string),
                    trace_token(line, "pending_choice_ids")
                        .filter(|value| *value != "-")
                        .map(|value| value.split(',').map(str::to_string).collect::<Vec<_>>())
                        .unwrap_or_default(),
                    trace_token(line, "terminal_route_ids")
                        .filter(|value| *value != "-")
                        .map(|value| value.split(',').map(str::to_string).collect::<Vec<_>>())
                        .unwrap_or_default(),
                    line,
                ),
            );
        }
    }
    consumed
        .into_iter()
        .filter_map(|(player_sequence, (kind, consumed_line))| {
            let (
                coverage_reached,
                fixed_step,
                session_id,
                current_state_id,
                pending_choice_ids,
                terminal_route_ids,
                runtime_line,
            ) = runtime.remove(&player_sequence)?;
            if trace_token(consumed_line, "session_id") != Some(session_id.as_str()) {
                return None;
            }
            Some(ParsedPlayerHostTrace {
                player_sequence,
                fixed_step,
                kind,
                trace_hash: Hash256::from_sha256(
                    format!("{consumed_line}\n{runtime_line}").as_bytes(),
                )
                .to_string(),
                coverage_reached,
                session_id,
                current_state_id,
                pending_choice_ids,
                terminal_route_ids,
            })
        })
        .collect()
}

#[cfg(any(test, all(target_os = "windows", feature = "platform-test-driver")))]
pub(super) fn trace_token<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}=");
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(&prefix))
}

#[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
pub(super) fn windows_live_input_impl(
    bundle: &BundleContext,
    timeout_ms: u64,
    trace_log: Option<&PathBuf>,
    inputs: &[ScenarioHostInput],
) -> Result<LiveInputRun, PlayerAutomationError> {
    windows_live::run(bundle, timeout_ms, trace_log, inputs)
}
