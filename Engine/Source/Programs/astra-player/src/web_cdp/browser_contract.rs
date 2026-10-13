use super::*;

pub const WEB_PLAYER_EVIDENCE_PREFIX: &str = "ASTRA_PLAYER_EVIDENCE ";

pub(super) const MAX_HTTP_REQUEST_BYTES: usize = 16 * 1024;

pub(super) const MAX_HTTP_RESPONSE_BYTES: u64 = 512 * 1024 * 1024;

pub(super) const MAX_CDP_DISCOVERY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct WebBrowserSessionRequest {
    pub browser_executable: PathBuf,
    pub bundle_dir: PathBuf,
    pub headless: bool,
    pub timeout: Duration,
}

pub struct WebBrowserSession {
    pub cdp: WebCdpSession,
    pub(super) child: Child,
    pub(super) _server: BundleHttpServer,
    pub(super) _profile: tempfile::TempDir,
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

pub(super) struct BundleHttpServer {
    pub(super) address: std::net::SocketAddr,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) thread: Option<JoinHandle<()>>,
}

impl BundleHttpServer {
    pub(super) fn start(root: &Path) -> Result<Self, PlayerAutomationError> {
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

    pub(super) fn address(&self) -> std::net::SocketAddr {
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

pub(super) fn serve_bundle_request(root: &Path, mut stream: TcpStream) -> std::io::Result<()> {
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

pub(super) fn write_http_error(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
) -> std::io::Result<()> {
    let body = format!("{status} {reason}\n");
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

pub(super) fn wait_for_debug_port(
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

pub(super) fn wait_for_page_target(
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

pub(super) fn read_cdp_target_list(port: u16) -> Result<Vec<u8>, PlayerAutomationError> {
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
    pub session_id: Option<String>,
    pub player_sequence: Option<u64>,
    pub input_kind: Option<String>,
    pub fixed_step: Option<u64>,
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
