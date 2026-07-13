use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use base64::Engine;
use serde_json::{json, Value};
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

use crate::{
    sha256_bytes, BundleContext, LiveInputRun, PlayerAutomationError,
    PlayerInputConsumptionEvidence, PlayerInputEvent, PlayerRuntimeRouteEvidence,
    PlayerVisualRegionEvidence, ScenarioInputAction, WEB_CDP_KEYBOARD,
};

pub const WEB_PLAYER_EVIDENCE_PREFIX: &str = "ASTRA_PLAYER_EVIDENCE ";
pub use astra_player_core::WEB_PLAYER_LIVE_EVIDENCE_SCHEMA;

const MAX_HTTP_REQUEST_BYTES: usize = 16 * 1024;
const MAX_HTTP_RESPONSE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_CDP_DISCOVERY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct WebBrowserSessionRequest {
    pub browser_executable: PathBuf,
    pub bundle_dir: PathBuf,
    pub headless: bool,
    pub timeout: Duration,
}

pub struct WebBrowserSession {
    pub cdp: WebCdpSession,
    child: Child,
    _server: BundleHttpServer,
    _profile: tempfile::TempDir,
}

impl WebBrowserSession {
    pub fn launch(request: WebBrowserSessionRequest) -> Result<Self, PlayerAutomationError> {
        if request.timeout.is_zero() {
            return Err("ASTRA_PLAYER_BROWSER_TIMEOUT_INVALID".into());
        }
        if !request.browser_executable.is_file() {
            return Err("ASTRA_PLAYER_BROWSER_EXECUTABLE_MISSING".into());
        }
        if !request.bundle_dir.join("index.html").is_file() {
            return Err("ASTRA_PLAYER_WEB_BUNDLE_ENTRYPOINT_MISSING".into());
        }
        let server = BundleHttpServer::start(&request.bundle_dir)?;
        let profile = tempfile::tempdir()
            .map_err(|error| format!("ASTRA_PLAYER_BROWSER_PROFILE_CREATE: {error}"))?;
        let page_url = format!("http://{}/", server.address());
        let mut command = Command::new(&request.browser_executable);
        command
            .arg("--remote-debugging-port=0")
            .arg("--remote-debugging-address=127.0.0.1")
            .arg(format!("--user-data-dir={}", profile.path().display()))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-background-networking")
            .arg("--disable-component-update")
            .arg("--disable-sync")
            .arg("--window-size=1280,720")
            .arg(&page_url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if request.headless {
            command.arg("--headless=new");
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("ASTRA_PLAYER_BROWSER_LAUNCH: {error}"))?;
        let cdp_result = (|| {
            let port = wait_for_debug_port(&mut child, profile.path(), request.timeout)?;
            let websocket_url = wait_for_page_target(port, &page_url, request.timeout)?;
            let mut cdp = WebCdpSession::connect(&websocket_url, request.timeout)?;
            cdp.bring_to_front(request.timeout)?;
            Ok::<_, PlayerAutomationError>(cdp)
        })();
        let cdp = match cdp_result {
            Ok(cdp) => cdp,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        Ok(Self {
            cdp,
            child,
            _server: server,
            _profile: profile,
        })
    }
}

impl Drop for WebBrowserSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct BundleHttpServer {
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl BundleHttpServer {
    fn start(root: &Path) -> Result<Self, PlayerAutomationError> {
        let root = root
            .canonicalize()
            .map_err(|error| format!("ASTRA_PLAYER_WEB_BUNDLE_ROOT: {error}"))?;
        if !root.is_dir() {
            return Err("ASTRA_PLAYER_WEB_BUNDLE_ROOT_INVALID".into());
        }
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("astra-web-bundle-http".to_string())
            .spawn(move || {
                while !thread_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let _ = serve_bundle_request(&root, stream);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
            })
            .map_err(|error| format!("ASTRA_PLAYER_WEB_SERVER_THREAD: {error}"))?;
        Ok(Self {
            address,
            stop,
            thread: Some(thread),
        })
    }

    fn address(&self) -> std::net::SocketAddr {
        self.address
    }
}

impl Drop for BundleHttpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve_bundle_request(root: &Path, mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return write_http_error(&mut stream, 400, "Bad Request");
        }
        request.extend_from_slice(&chunk[..read]);
        if request.len() > MAX_HTTP_REQUEST_BYTES {
            return write_http_error(&mut stream, 413, "Payload Too Large");
        }
    }
    let request = match std::str::from_utf8(&request) {
        Ok(request) => request,
        Err(_) => return write_http_error(&mut stream, 400, "Bad Request"),
    };
    let Some(line) = request.lines().next() else {
        return write_http_error(&mut stream, 400, "Bad Request");
    };
    let mut fields = line.split_ascii_whitespace();
    let (Some(method), Some(target), Some(version), None) =
        (fields.next(), fields.next(), fields.next(), fields.next())
    else {
        return write_http_error(&mut stream, 400, "Bad Request");
    };
    if method != "GET" {
        return write_http_error(&mut stream, 405, "Method Not Allowed");
    }
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return write_http_error(&mut stream, 400, "Bad Request");
    }
    let target = target.split('?').next().unwrap_or_default();
    if !target.starts_with('/') || target.contains('%') || target.contains('\\') {
        return write_http_error(&mut stream, 400, "Bad Request");
    }
    let relative = if target == "/" {
        PathBuf::from("index.html")
    } else {
        PathBuf::from(&target[1..])
    };
    if relative.as_os_str().is_empty()
        || relative.components().any(|component| {
            !matches!(component, Component::Normal(_))
                || component.as_os_str().to_string_lossy().contains(':')
        })
    {
        return write_http_error(&mut stream, 400, "Bad Request");
    }
    let path = match root.join(relative).canonicalize() {
        Ok(path) if path.starts_with(root) => path,
        _ => return write_http_error(&mut stream, 404, "Not Found"),
    };
    let metadata = match fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() => metadata,
        _ => return write_http_error(&mut stream, 404, "Not Found"),
    };
    if metadata.len() > MAX_HTTP_RESPONSE_BYTES {
        return write_http_error(&mut stream, 413, "Payload Too Large");
    }
    let body = fs::read(&path)?;
    let mime = match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json; charset=utf-8",
        Some("yaml" | "yml") => "application/yaml; charset=utf-8",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    };
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: {mime}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nCross-Origin-Opener-Policy: same-origin\r\nCross-Origin-Embedder-Policy: require-corp\r\nCross-Origin-Resource-Policy: same-origin\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)
}

fn write_http_error(stream: &mut TcpStream, status: u16, reason: &str) -> std::io::Result<()> {
    let body = format!("{status} {reason}\n");
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn wait_for_debug_port(
    child: &mut Child,
    profile: &Path,
    timeout: Duration,
) -> Result<u16, PlayerAutomationError> {
    let deadline = Instant::now() + timeout;
    let port_file = profile.join("DevToolsActivePort");
    loop {
        if let Some(status) = child.try_wait()? {
            return Err(format!("ASTRA_PLAYER_BROWSER_EXITED: {status}").into());
        }
        if let Ok(contents) = fs::read_to_string(&port_file) {
            let mut lines = contents.lines();
            let port = lines
                .next()
                .and_then(|line| line.parse::<u16>().ok())
                .filter(|port| *port != 0)
                .ok_or("ASTRA_PLAYER_BROWSER_DEBUG_PORT_INVALID")?;
            let browser_path = lines
                .next()
                .filter(|line| line.starts_with("/devtools/browser/") && line.len() > 20)
                .ok_or("ASTRA_PLAYER_BROWSER_DEBUG_TARGET_INVALID")?;
            if lines.next().is_some() || browser_path.bytes().any(|byte| byte.is_ascii_whitespace())
            {
                return Err("ASTRA_PLAYER_BROWSER_DEBUG_TARGET_INVALID".into());
            }
            return Ok(port);
        }
        if Instant::now() >= deadline {
            return Err("ASTRA_PLAYER_BROWSER_DEBUG_PORT_TIMEOUT".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_page_target(
    port: u16,
    page_url: &str,
    timeout: Duration,
) -> Result<String, PlayerAutomationError> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(body) = read_cdp_target_list(port) {
            let targets: Vec<Value> = serde_json::from_slice(&body)
                .map_err(|error| format!("ASTRA_PLAYER_CDP_DISCOVERY_JSON: {error}"))?;
            let matching: Vec<&Value> = targets
                .iter()
                .filter(|target| {
                    target.get("type").and_then(Value::as_str) == Some("page")
                        && target.get("url").and_then(Value::as_str) == Some(page_url)
                })
                .collect();
            if matching.len() > 1 {
                return Err("ASTRA_PLAYER_CDP_PAGE_TARGET_AMBIGUOUS".into());
            }
            if let Some(target) = matching.first() {
                let url = target
                    .get("webSocketDebuggerUrl")
                    .and_then(Value::as_str)
                    .ok_or("ASTRA_PLAYER_CDP_PAGE_WEBSOCKET_MISSING")?;
                let prefix = format!("ws://127.0.0.1:{port}/devtools/page/");
                if !url.starts_with(&prefix) || url.len() <= prefix.len() {
                    return Err("ASTRA_PLAYER_CDP_PAGE_WEBSOCKET_INVALID".into());
                }
                return Ok(url.to_string());
            }
        }
        if Instant::now() >= deadline {
            return Err("ASTRA_PLAYER_CDP_PAGE_TARGET_TIMEOUT".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn read_cdp_target_list(port: u16) -> Result<Vec<u8>, PlayerAutomationError> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    write!(
        stream,
        "GET /json/list HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = Vec::new();
    stream
        .take((MAX_CDP_DISCOVERY_BYTES + 1) as u64)
        .read_to_end(&mut response)?;
    if response.len() > MAX_CDP_DISCOVERY_BYTES {
        return Err("ASTRA_PLAYER_CDP_DISCOVERY_TOO_LARGE".into());
    }
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or("ASTRA_PLAYER_CDP_DISCOVERY_HTTP_INVALID")?;
    let (headers, body_with_separator) = response.split_at(header_end);
    let headers =
        std::str::from_utf8(headers).map_err(|_| "ASTRA_PLAYER_CDP_DISCOVERY_HTTP_INVALID")?;
    if !headers.starts_with("HTTP/1.1 200 ") && !headers.starts_with("HTTP/1.0 200 ") {
        return Err("ASTRA_PLAYER_CDP_DISCOVERY_HTTP_STATUS".into());
    }
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .ok_or("ASTRA_PLAYER_CDP_DISCOVERY_LENGTH_MISSING")?;
    let body = &body_with_separator[4..];
    if content_length != body.len() {
        return Err("ASTRA_PLAYER_CDP_DISCOVERY_LENGTH_MISMATCH".into());
    }
    Ok(body.to_vec())
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WebCdpRuntimeEvidence {
    pub event: String,
    pub target: String,
    pub profile: String,
    pub package_hash: String,
    pub provider_id: Option<String>,
    pub session_id: Option<String>,
    pub player_sequence: Option<u64>,
    pub input_kind: Option<String>,
    pub fixed_step: Option<u64>,
    pub runtime_state_hash: Option<String>,
    pub runtime_event_hash: Option<String>,
    pub runtime_presentation_hash: Option<String>,
    pub coverage_reached: Vec<String>,
    pub current_state_id: Option<String>,
    pub terminal_route_ids: Vec<String>,
    pub pending_choice_ids: Vec<String>,
    pub audio_meter: Option<Value>,
}

pub(crate) struct WebLiveInputRun {
    pub live: LiveInputRun,
    pub final_evidence: WebCdpRuntimeEvidence,
}

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
            runtime_state_hash: required_hash(&evidence.runtime_state_hash, "state")?,
            runtime_event_hash: required_hash(&evidence.runtime_event_hash, "event")?,
            runtime_presentation_hash: required_hash(
                &evidence.runtime_presentation_hash,
                "presentation",
            )?,
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

fn wait_for_evidence(
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

fn validate_evidence_identity(
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

fn required_hash(value: &Option<String>, domain: &str) -> Result<String, PlayerAutomationError> {
    value
        .as_ref()
        .filter(|value| value.starts_with("hash128:"))
        .cloned()
        .ok_or_else(|| format!("ASTRA_PLAYER_WEB_RUNTIME_HASH_MISSING: {domain}").into())
}

fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32), PlayerAutomationError> {
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
    fn parse(value: Value) -> Result<Self, PlayerAutomationError> {
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
            provider_id: optional_string("provider_id"),
            session_id: optional_string("session_id"),
            player_sequence: value.get("player_sequence").and_then(Value::as_u64),
            input_kind: optional_string("input_kind"),
            fixed_step: value.get("fixed_step").and_then(Value::as_u64),
            runtime_state_hash: optional_string("runtime_state_hash"),
            runtime_event_hash: optional_string("runtime_event_hash"),
            runtime_presentation_hash: optional_string("runtime_presentation_hash"),
            coverage_reached: string_array("coverage_reached")?,
            current_state_id: optional_string("current_state_id"),
            terminal_route_ids: string_array("terminal_route_ids")?,
            pending_choice_ids: string_array("pending_choice_ids")?,
            audio_meter: value.get("audio_meter").cloned(),
        })
    }
}

pub struct WebCdpSession {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: u64,
    responses: BTreeMap<u64, Value>,
    events: VecDeque<Value>,
}

impl WebCdpSession {
    pub fn connect(websocket_url: &str, timeout: Duration) -> Result<Self, PlayerAutomationError> {
        if timeout.is_zero() {
            return Err("ASTRA_PLAYER_CDP_TIMEOUT_INVALID: timeout must be non-zero".into());
        }
        let (mut socket, _) = tungstenite::connect(websocket_url)
            .map_err(|error| format!("ASTRA_PLAYER_CDP_CONNECT: {error}"))?;
        match socket.get_mut() {
            MaybeTlsStream::Plain(stream) => {
                stream.set_read_timeout(Some(timeout))?;
                stream.set_write_timeout(Some(timeout))?;
            }
            _ => {
                return Err(
                    "ASTRA_PLAYER_CDP_TRANSPORT: only local ws transport is allowed".into(),
                );
            }
        }
        let mut session = Self {
            socket,
            next_id: 0,
            responses: BTreeMap::new(),
            events: VecDeque::new(),
        };
        session.call("Runtime.enable", json!({}), timeout)?;
        session.call("Log.enable", json!({}), timeout)?;
        session.call("Page.enable", json!({}), timeout)?;
        Ok(session)
    }

    pub fn call(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, PlayerAutomationError> {
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or("ASTRA_PLAYER_CDP_SEQUENCE_OVERFLOW")?;
        let id = self.next_id;
        self.socket
            .send(Message::Text(
                serde_json::to_string(&json!({"id": id, "method": method, "params": params}))?
                    .into(),
            ))
            .map_err(|error| format!("ASTRA_PLAYER_CDP_SEND: {error}"))?;
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(response) = self.responses.remove(&id) {
                if let Some(error) = response.get("error") {
                    return Err(format!("ASTRA_PLAYER_CDP_COMMAND: {method}: {error}").into());
                }
                return response
                    .get("result")
                    .cloned()
                    .ok_or_else(|| "ASTRA_PLAYER_CDP_RESULT_MISSING".into());
            }
            self.read_one(deadline)?;
        }
    }

    pub fn dispatch_key(
        &mut self,
        key: &str,
        code: &str,
        virtual_key: u32,
        timeout: Duration,
    ) -> Result<(), PlayerAutomationError> {
        for event_type in ["keyDown", "keyUp"] {
            self.call(
                "Input.dispatchKeyEvent",
                json!({
                    "type": event_type,
                    "key": key,
                    "code": code,
                    "windowsVirtualKeyCode": virtual_key,
                    "nativeVirtualKeyCode": virtual_key,
                }),
                timeout,
            )?;
        }
        Ok(())
    }

    pub fn bring_to_front(&mut self, timeout: Duration) -> Result<(), PlayerAutomationError> {
        self.call("Page.bringToFront", json!({}), timeout)?;
        Ok(())
    }

    pub fn launch_button_center(
        &mut self,
        timeout: Duration,
    ) -> Result<(f64, f64), PlayerAutomationError> {
        let result = self.call(
            "Runtime.evaluate",
            json!({
                "expression": "(() => { const button = document.querySelector('button[data-astra-permission-handshake]'); if (!button || button.disabled) return null; const rect = button.getBoundingClientRect(); return {x: rect.x + rect.width / 2, y: rect.y + rect.height / 2, status: button.dataset.astraPermissionHandshake}; })()",
                "returnByValue": true,
                "awaitPromise": false,
            }),
            timeout,
        )?;
        let value = result
            .pointer("/result/value")
            .ok_or("ASTRA_PLAYER_CDP_LAUNCH_BUTTON_MISSING")?;
        if value.get("status").and_then(Value::as_str) != Some("pending") {
            return Err("ASTRA_PLAYER_CDP_LAUNCH_STATE_INVALID".into());
        }
        let x = value
            .get("x")
            .and_then(Value::as_f64)
            .ok_or("ASTRA_PLAYER_CDP_LAUNCH_BUTTON_INVALID")?;
        let y = value
            .get("y")
            .and_then(Value::as_f64)
            .ok_or("ASTRA_PLAYER_CDP_LAUNCH_BUTTON_INVALID")?;
        if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 {
            return Err("ASTRA_PLAYER_CDP_LAUNCH_BUTTON_INVALID".into());
        }
        Ok((x, y))
    }

    pub fn canvas_center(
        &mut self,
        timeout: Duration,
    ) -> Result<(f64, f64), PlayerAutomationError> {
        let result = self.call(
            "Runtime.evaluate",
            json!({
                "expression": "(() => { const canvas = document.querySelector('canvas#astra-player'); if (!canvas) return null; const rect = canvas.getBoundingClientRect(); if (rect.width <= 0 || rect.height <= 0) return null; return {x: rect.x + rect.width / 2, y: rect.y + rect.height / 2}; })()",
                "returnByValue": true,
                "awaitPromise": false,
            }),
            timeout,
        )?;
        let value = result
            .pointer("/result/value")
            .ok_or("ASTRA_PLAYER_CDP_CANVAS_MISSING")?;
        let x = value
            .get("x")
            .and_then(Value::as_f64)
            .ok_or("ASTRA_PLAYER_CDP_CANVAS_INVALID")?;
        let y = value
            .get("y")
            .and_then(Value::as_f64)
            .ok_or("ASTRA_PLAYER_CDP_CANVAS_INVALID")?;
        if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 {
            return Err("ASTRA_PLAYER_CDP_CANVAS_INVALID".into());
        }
        Ok((x, y))
    }

    pub fn dispatch_mouse_click(
        &mut self,
        x: f64,
        y: f64,
        timeout: Duration,
    ) -> Result<(), PlayerAutomationError> {
        if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 {
            return Err("ASTRA_PLAYER_CDP_COORDINATE_INVALID".into());
        }
        self.call(
            "Input.dispatchMouseEvent",
            json!({"type": "mouseMoved", "x": x, "y": y, "pointerType": "mouse"}),
            timeout,
        )?;
        self.call(
            "Input.dispatchMouseEvent",
            json!({"type": "mousePressed", "x": x, "y": y, "button": "left", "buttons": 1, "clickCount": 1, "pointerType": "mouse"}),
            timeout,
        )?;
        self.call(
            "Input.dispatchMouseEvent",
            json!({"type": "mouseReleased", "x": x, "y": y, "button": "left", "buttons": 0, "clickCount": 1, "pointerType": "mouse"}),
            timeout,
        )?;
        Ok(())
    }

    pub fn capture_screenshot(
        &mut self,
        timeout: Duration,
    ) -> Result<Vec<u8>, PlayerAutomationError> {
        let result = self.call(
            "Page.captureScreenshot",
            json!({"format": "png", "fromSurface": true, "captureBeyondViewport": false}),
            timeout,
        )?;
        let encoded = result
            .get("data")
            .and_then(Value::as_str)
            .ok_or("ASTRA_PLAYER_CDP_SCREENSHOT_MISSING")?;
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| format!("ASTRA_PLAYER_CDP_SCREENSHOT_INVALID: {error}").into())
    }

    pub fn wait_for_runtime_evidence(
        &mut self,
        timeout: Duration,
    ) -> Result<WebCdpRuntimeEvidence, PlayerAutomationError> {
        let deadline = Instant::now() + timeout;
        loop {
            while let Some(event) = self.events.pop_front() {
                if let Some(evidence) = parse_console_evidence(&event)? {
                    return Ok(evidence);
                }
                if event.get("method").and_then(Value::as_str) == Some("Runtime.exceptionThrown") {
                    return Err("ASTRA_PLAYER_CDP_RUNTIME_EXCEPTION: browser runtime failed".into());
                }
            }
            self.read_one(deadline)?;
        }
    }

    fn read_one(&mut self, deadline: Instant) -> Result<(), PlayerAutomationError> {
        if Instant::now() >= deadline {
            return Err("ASTRA_PLAYER_CDP_TIMEOUT".into());
        }
        let message = self
            .socket
            .read()
            .map_err(|error| format!("ASTRA_PLAYER_CDP_READ: {error}"))?;
        let Message::Text(text) = message else {
            return match message {
                Message::Ping(payload) => {
                    self.socket.send(Message::Pong(payload))?;
                    Ok(())
                }
                Message::Pong(_) => Ok(()),
                Message::Close(_) => Err("ASTRA_PLAYER_CDP_CLOSED".into()),
                _ => Err("ASTRA_PLAYER_CDP_MESSAGE_UNSUPPORTED".into()),
            };
        };
        let value: Value = serde_json::from_str(text.as_str())?;
        if let Some(id) = value.get("id").and_then(Value::as_u64) {
            if self.responses.insert(id, value).is_some() {
                return Err("ASTRA_PLAYER_CDP_DUPLICATE_RESPONSE".into());
            }
        } else if value.get("method").and_then(Value::as_str).is_some() {
            self.events.push_back(value);
        } else {
            return Err("ASTRA_PLAYER_CDP_MESSAGE_INVALID".into());
        }
        Ok(())
    }
}

fn parse_console_evidence(
    event: &Value,
) -> Result<Option<WebCdpRuntimeEvidence>, PlayerAutomationError> {
    if event.get("method").and_then(Value::as_str) != Some("Runtime.consoleAPICalled") {
        return Ok(None);
    }
    let args = event
        .pointer("/params/args")
        .and_then(Value::as_array)
        .ok_or("ASTRA_PLAYER_CDP_CONSOLE_ARGS_MISSING")?;
    for argument in args {
        let Some(text) = argument.get("value").and_then(Value::as_str) else {
            continue;
        };
        let Some(encoded) = text.strip_prefix(WEB_PLAYER_EVIDENCE_PREFIX) else {
            continue;
        };
        return Ok(Some(WebCdpRuntimeEvidence::parse(serde_json::from_str(
            encoded,
        )?)?));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(address: std::net::SocketAddr, target: &str) -> Vec<u8> {
        let mut stream = TcpStream::connect(address).expect("connect bundle server");
        write!(
            stream,
            "GET {target} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        )
        .expect("write request");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).expect("read response");
        response
    }

    #[test]
    fn bundle_server_serves_entrypoint_with_required_isolation_headers() {
        let root = tempfile::tempdir().expect("bundle root");
        fs::write(root.path().join("index.html"), b"astra").expect("write entrypoint");
        let server = BundleHttpServer::start(root.path()).expect("start server");
        let response = request(server.address(), "/");
        let response = String::from_utf8(response).expect("utf8 response");
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("Content-Type: text/html; charset=utf-8\r\n"));
        assert!(response.contains("Cross-Origin-Opener-Policy: same-origin\r\n"));
        assert!(response.contains("Cross-Origin-Embedder-Policy: require-corp\r\n"));
        assert!(response.ends_with("\r\n\r\nastra"));
    }

    #[test]
    fn bundle_server_rejects_encoded_and_parent_traversal() {
        let root = tempfile::tempdir().expect("bundle root");
        fs::write(root.path().join("index.html"), b"astra").expect("write entrypoint");
        let server = BundleHttpServer::start(root.path()).expect("start server");
        for target in ["/%2e%2e/secret", "/../secret", "/C:/secret"] {
            let response = request(server.address(), target);
            assert!(
                response.starts_with(b"HTTP/1.1 400 Bad Request\r\n"),
                "target {target} was not rejected"
            );
        }
    }
}
