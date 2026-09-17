use super::*;
use crate::{DiagnosticSink, DiagnosticSink_TO};
use abi_stable::sabi_trait::TD_Opaque;
use std::sync::{Arc, Mutex};

struct Recorder(Arc<Mutex<Vec<DiagnosticEvent>>>);
impl DiagnosticSink for Recorder {
    fn enabled(&self, level: DiagnosticLevel) -> bool {
        level != DiagnosticLevel::Trace
    }
    fn emit(&self, event: DiagnosticEvent) {
        self.0.lock().unwrap().push(event);
    }
}

#[test]
fn strings_messages_and_debug_are_forwarded_without_redaction() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = DiagnosticSink_TO::from_value(Recorder(events.clone()), TD_Opaque);
    let subscriber = tracing_subscriber::registry().with(Bridge { sink });
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(target: "core/plugin", event = "core.ready", path = "fixture/game.dat",
            description = "中文日志", backend = %"custom GPU", details = ?vec![1, 2], count = 4_u64);
        tracing::warn!(target: "core", "native message {}", 7);
        tracing::trace!(target: "core", "disabled");
    });
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].target, "core/plugin");
    for (name, value) in [
        ("path", "fixture/game.dat"),
        ("description", "中文日志"),
        ("backend", "custom GPU"),
        ("details", "[1, 2]"),
    ] {
        assert!(events[0]
            .fields
            .iter()
            .any(|field| field.name == name && field.value == DiagnosticValue::Text(value.into())));
    }
    assert!(events[1].fields.iter().any(|field| field.name == "message"));
    for event in events.iter() {
        assert_eq!(event.dropped_fields, 0);
        event.validate().unwrap();
    }
}

#[test]
fn text_bounds_and_invalid_numeric_values_remain_observable() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = DiagnosticSink_TO::from_value(Recorder(events.clone()), TD_Opaque);
    let subscriber = tracing_subscriber::registry().with(Bridge { sink });
    let oversized = "x".repeat(MAX_DIAGNOSTIC_TEXT_BYTES + 1);
    tracing::subscriber::with_default(subscriber, || {
        tracing::warn!(event = "core.large", text = oversized.as_str(), detail = ?oversized, value = f64::NAN);
    });
    let events = events.lock().unwrap();
    assert_eq!(events[0].dropped_fields, 3);
    events[0].validate().unwrap();
    let mut malformed = events[0].clone();
    malformed.fields.push(DiagnosticField {
        name: "text".into(),
        value: DiagnosticValue::Text(oversized.into()),
    });
    assert!(malformed.validate().is_err());
}
