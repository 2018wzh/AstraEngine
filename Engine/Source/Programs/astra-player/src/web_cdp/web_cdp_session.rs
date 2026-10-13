use super::*;

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

    pub(super) fn read_one(&mut self, deadline: Instant) -> Result<(), PlayerAutomationError> {
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

pub(super) fn parse_console_evidence(
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
