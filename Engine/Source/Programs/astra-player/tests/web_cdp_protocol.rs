use std::{net::TcpListener, thread, time::Duration};

use astra_player::{WebCdpSession, WEB_CDP_KEYBOARD, WEB_PLAYER_LIVE_EVIDENCE_SCHEMA};
use serde_json::{json, Value};
use tungstenite::Message;

#[test]
fn cdp_session_consumes_runtime_owned_evidence_and_real_protocol_results() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        for expected in ["Runtime.enable", "Log.enable", "Page.enable"] {
            let request = read_request(&mut socket);
            assert_eq!(request["method"], expected);
            reply_ok(&mut socket, request["id"].as_u64().unwrap(), json!({}));
        }
        socket
            .send(Message::Text(
                serde_json::to_string(&json!({
                    "method": "Runtime.consoleAPICalled",
                    "params": {"args": [{
                        "type": "string",
                        "value": format!("ASTRA_PLAYER_EVIDENCE {}", serde_json::to_string(&json!({
                            "schema": WEB_PLAYER_LIVE_EVIDENCE_SCHEMA,
                            "event": "runtime.input_consumed",
                            "target": "nativevn-game",
                            "profile": "classic",
                            "package_hash": format!("sha256:{}", "1".repeat(64)),
                            "provider_id": "astra.runtime.native_vn",
                            "session_id": "session.web.1",
                            "player_sequence": 4,
                            "input_kind": WEB_CDP_KEYBOARD,
                            "fixed_step": 2,
                            "runtime_state_hash": format!("sha256:{}", "2".repeat(64)),
                            "runtime_event_hash": format!("sha256:{}", "3".repeat(64)),
                            "runtime_presentation_hash": format!("sha256:{}", "4".repeat(64)),
                            "coverage_reached": ["route.library"],
                            "current_state_id": "route.library",
                            "terminal_route_ids": ["route.library"],
                            "pending_choice_ids": [],
                            "audio_meter": null
                        })).unwrap())
                    }]}
                }))
                .unwrap()
                .into(),
            ))
            .unwrap();
        let request = read_request(&mut socket);
        assert_eq!(request["method"], "Page.captureScreenshot");
        reply_ok(
            &mut socket,
            request["id"].as_u64().unwrap(),
            json!({"data": "iVBORw0KGgo="}),
        );
    });

    let mut session =
        WebCdpSession::connect(&format!("ws://{address}"), Duration::from_secs(2)).unwrap();
    let evidence = session
        .wait_for_runtime_evidence(Duration::from_secs(2))
        .unwrap();
    assert_eq!(evidence.session_id.as_deref(), Some("session.web.1"));
    assert_eq!(evidence.terminal_route_ids, ["route.library"]);
    assert_eq!(
        session.capture_screenshot(Duration::from_secs(2)).unwrap(),
        b"\x89PNG\r\n\x1a\n"
    );
    server.join().unwrap();
}

fn read_request(socket: &mut tungstenite::WebSocket<std::net::TcpStream>) -> Value {
    match socket.read().unwrap() {
        Message::Text(text) => serde_json::from_str(text.as_str()).unwrap(),
        message => panic!("unexpected CDP message: {message:?}"),
    }
}

fn reply_ok(socket: &mut tungstenite::WebSocket<std::net::TcpStream>, id: u64, result: Value) {
    socket
        .send(Message::Text(
            serde_json::to_string(&json!({"id": id, "result": result}))
                .unwrap()
                .into(),
        ))
        .unwrap();
}
