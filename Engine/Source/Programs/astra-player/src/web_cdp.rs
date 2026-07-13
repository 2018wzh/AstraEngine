use std::{
    collections::{BTreeMap, VecDeque},
    net::TcpStream,
    time::{Duration, Instant},
};

use base64::Engine;
use serde_json::{json, Value};
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

use crate::PlayerAutomationError;

pub const WEB_PLAYER_EVIDENCE_PREFIX: &str = "ASTRA_PLAYER_EVIDENCE ";
pub use astra_player_core::WEB_PLAYER_LIVE_EVIDENCE_SCHEMA;

#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub terminal_route_ids: Vec<String>,
    pub pending_choice_ids: Vec<String>,
    pub audio_meter: Option<Value>,
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
