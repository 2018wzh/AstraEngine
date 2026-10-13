use super::*;

pub(crate) fn run_web_live_input(
    bundle: &BundleContext,
    inputs: &[ScenarioInputAction],
    browser_executable: PathBuf,
    headless: bool,
    timeout: Duration,
) -> Result<WebLiveInputRun, PlayerAutomationError> {
    let mut browser = WebBrowserSession::launch(WebBrowserSessionRequest {
        browser_executable,
        bundle_dir: bundle.bundle_dir.clone(),
        headless,
        timeout,
    })?;
    let (launch_x, launch_y) = browser.cdp.launch_button_center(timeout)?;
    browser
        .cdp
        .dispatch_mouse_click(launch_x, launch_y, timeout)?;
    let package_evidence = wait_for_evidence(&mut browser.cdp, timeout, |evidence| {
        evidence.event == "package.validated"
    })?;
    validate_evidence_identity(bundle, &package_evidence)?;
    let mut previous_png = browser.cdp.capture_screenshot(timeout)?;
    let (width, height) = png_dimensions(&previous_png)?;
    let mut events = Vec::with_capacity(inputs.len());
    let mut consumption = Vec::with_capacity(inputs.len());
    let mut visual_regions = Vec::with_capacity(inputs.len());
    let mut runtime_routes = Vec::with_capacity(inputs.len());
    let mut route_coverage = std::collections::BTreeSet::new();
    let mut last_evidence = None;
    let mut previous_player_sequence = 0;
    let mut previous_fixed_step = 0;
    let mut run_session_id: Option<String> = None;

    for (index, input) in inputs.iter().enumerate() {
        let (key, code, virtual_key) = match input {
            ScenarioInputAction::Advance => ("Enter", "Enter", 0x0D),
            ScenarioInputAction::Back => ("Escape", "Escape", 0x1B),
            ScenarioInputAction::OpenSystem { page } if page == "backlog" => ("b", "KeyB", 0x42),
            ScenarioInputAction::OpenSystem { page } => {
                return Err(format!(
                    "ASTRA_PLAYER_SYSTEM_KEY_UNAVAILABLE: no browser binding for {page}"
                )
                .into());
            }
            ScenarioInputAction::Choose { option_id } => {
                let pending = last_evidence
                    .as_ref()
                    .map(|evidence: &WebCdpRuntimeEvidence| evidence.pending_choice_ids.as_slice())
                    .unwrap_or_default();
                let matches = pending
                    .iter()
                    .enumerate()
                    .filter(|(_, id)| *id == option_id)
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                if matches.len() != 1 || matches[0] >= 9 {
                    return Err(format!(
                        "ASTRA_PLAYER_WEB_CHOICE_NOT_PENDING: {option_id} is not uniquely keyboard-selectable"
                    )
                    .into());
                }
                let digit = (b'1' + matches[0] as u8) as char;
                let key = digit.to_string();
                let code = format!("Digit{digit}");
                browser
                    .cdp
                    .dispatch_key(&key, &code, 0x31 + matches[0] as u32, timeout)?;
                ("", "", 0)
            }
        };
        if !key.is_empty() {
            browser.cdp.dispatch_key(key, code, virtual_key, timeout)?;
        }
        let evidence = wait_for_evidence(&mut browser.cdp, timeout, |evidence| {
            evidence.event == "runtime.input_consumed"
                && evidence.player_sequence.unwrap_or_default() > previous_player_sequence
        })?;
        validate_evidence_identity(bundle, &evidence)?;
        let session_id = evidence
            .session_id
            .as_ref()
            .filter(|id| !id.is_empty())
            .ok_or("ASTRA_PLAYER_WEB_SESSION_ID_MISSING")?;
        if run_session_id.as_ref().is_some_and(|id| id != session_id) {
            return Err("ASTRA_PLAYER_WEB_SESSION_ID_MISMATCH".into());
        }
        run_session_id.get_or_insert_with(|| session_id.clone());
        let player_sequence = evidence
            .player_sequence
            .ok_or("ASTRA_PLAYER_WEB_PLAYER_SEQUENCE_MISSING")?;
        let fixed_step = evidence
            .fixed_step
            .ok_or("ASTRA_PLAYER_WEB_FIXED_STEP_MISSING")?;
        if player_sequence <= previous_player_sequence || fixed_step <= previous_fixed_step {
            return Err("ASTRA_PLAYER_WEB_RUNTIME_ORDER_INVALID".into());
        }
        let expected_input_kind = "keyboard";
        if evidence.input_kind.as_deref() != Some(expected_input_kind) {
            return Err("ASTRA_PLAYER_WEB_INPUT_KIND_MISMATCH".into());
        }
        let input_sequence = (index + 1) as u64;
        let trace_hash = sha256_bytes(&serde_json::to_vec(&evidence)?);
        let route_id = evidence
            .current_state_id
            .clone()
            .or_else(|| evidence.coverage_reached.last().cloned());
        events.push(PlayerInputEvent {
            step_id: format!("input.{}.{}", input.kind(), index + 1),
            source: WEB_CDP_KEYBOARD.to_string(),
            kind: "key".to_string(),
            sequence: input_sequence,
            route_id: route_id.clone(),
        });
        consumption.push(PlayerInputConsumptionEvidence {
            input_sequence,
            player_sequence,
            source: "player_web.runtime_evidence".to_string(),
            kind: expected_input_kind.to_string(),
            trace_event: "astra.player.input.consumed".to_string(),
            trace_hash: trace_hash.clone(),
            route_id,
        });
        route_coverage.extend(evidence.coverage_reached.iter().cloned());
        runtime_routes.push(PlayerRuntimeRouteEvidence {
            input_sequence,
            player_sequence,
            fixed_step,
            coverage_reached: evidence.coverage_reached.clone(),
            current_state_id: evidence.current_state_id.clone(),
            pending_choice_ids: evidence.pending_choice_ids.clone(),
            terminal_route_ids: evidence.terminal_route_ids.clone(),
            session_id: session_id.clone(),
            trace_hash,
        });
        let after_png = browser.cdp.capture_screenshot(timeout)?;
        let (after_width, after_height) = png_dimensions(&after_png)?;
        visual_regions.push(PlayerVisualRegionEvidence {
            region_id: format!("browser_viewport.input.{}", index + 1),
            before_hash: sha256_bytes(&previous_png),
            after_hash: sha256_bytes(&after_png),
            width: width.min(after_width),
            height: height.min(after_height),
        });
        previous_png = after_png;
        previous_player_sequence = player_sequence;
        previous_fixed_step = fixed_step;
        last_evidence = Some(evidence);
    }
    let final_evidence = last_evidence.ok_or("ASTRA_PLAYER_WEB_RUNTIME_EVIDENCE_EMPTY")?;
    Ok(WebLiveInputRun {
        live: LiveInputRun {
            events,
            input_consumption: consumption,
            visual_regions,
            runtime_routes,
            route_coverage: route_coverage.into_iter().collect(),
        },
        final_evidence,
    })
}

pub(super) fn wait_for_evidence(
    cdp: &mut WebCdpSession,
    timeout: Duration,
    predicate: impl Fn(&WebCdpRuntimeEvidence) -> bool,
) -> Result<WebCdpRuntimeEvidence, PlayerAutomationError> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("ASTRA_PLAYER_CDP_EVIDENCE_TIMEOUT")?;
        let evidence = cdp.wait_for_runtime_evidence(remaining)?;
        if predicate(&evidence) {
            return Ok(evidence);
        }
    }
}

pub(super) fn validate_evidence_identity(
    bundle: &BundleContext,
    evidence: &WebCdpRuntimeEvidence,
) -> Result<(), PlayerAutomationError> {
    if evidence.target != bundle.target
        || evidence.profile != bundle.profile
        || evidence.package_hash != bundle.package_hash
    {
        return Err("ASTRA_PLAYER_WEB_EVIDENCE_IDENTITY_MISMATCH".into());
    }
    Ok(())
}

pub(super) fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32), PlayerAutomationError> {
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" {
        return Err("ASTRA_PLAYER_CDP_SCREENSHOT_PNG_INVALID".into());
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into()?);
    if width == 0 || height == 0 {
        return Err("ASTRA_PLAYER_CDP_SCREENSHOT_DIMENSIONS_INVALID".into());
    }
    Ok((width, height))
}

impl WebCdpRuntimeEvidence {
    pub(super) fn parse(value: Value) -> Result<Self, PlayerAutomationError> {
        if value.get("schema").and_then(Value::as_str) != Some(WEB_PLAYER_LIVE_EVIDENCE_SCHEMA) {
            return Err("ASTRA_PLAYER_WEB_EVIDENCE_SCHEMA: unsupported evidence schema".into());
        }
        let required = |key: &str| -> Result<String, PlayerAutomationError> {
            value
                .get(key)
                .and_then(Value::as_str)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .ok_or_else(|| format!("ASTRA_PLAYER_WEB_EVIDENCE_FIELD: missing {key}").into())
        };
        let optional_string =
            |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
        let string_array = |key: &str| -> Result<Vec<String>, PlayerAutomationError> {
            value
                .get(key)
                .and_then(Value::as_array)
                .ok_or_else(|| -> PlayerAutomationError {
                    format!("ASTRA_PLAYER_WEB_EVIDENCE_FIELD: missing array {key}").into()
                })?
                .iter()
                .map(|item| {
                    item.as_str()
                        .filter(|item| !item.is_empty())
                        .map(str::to_string)
                        .ok_or_else(|| -> PlayerAutomationError {
                            format!("ASTRA_PLAYER_WEB_EVIDENCE_FIELD: invalid {key} item").into()
                        })
                })
                .collect()
        };
        Ok(Self {
            event: required("event")?,
            target: required("target")?,
            profile: required("profile")?,
            package_hash: required("package_hash")?,
            session_id: optional_string("session_id"),
            player_sequence: value.get("player_sequence").and_then(Value::as_u64),
            input_kind: optional_string("input_kind"),
            fixed_step: value.get("fixed_step").and_then(Value::as_u64),
            coverage_reached: string_array("coverage_reached")?,
            current_state_id: optional_string("current_state_id"),
            terminal_route_ids: string_array("terminal_route_ids")?,
            pending_choice_ids: string_array("pending_choice_ids")?,
            audio_meter: value.get("audio_meter").cloned(),
        })
    }
}

pub struct WebCdpSession {
    pub(super) socket: WebSocket<MaybeTlsStream<TcpStream>>,
    pub(super) next_id: u64,
    pub(super) responses: BTreeMap<u64, Value>,
    pub(super) events: VecDeque<Value>,
}
