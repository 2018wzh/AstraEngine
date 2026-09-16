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
fn structured_diagnostics_preserve_numbers_and_never_format_private_debug() {
    struct Private;
    impl fmt::Debug for Private {
        fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
            panic!("private Debug must not be evaluated")
        }
    }
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = DiagnosticSink_TO::from_value(Recorder(events.clone()), TD_Opaque);
    let subscriber = tracing_subscriber::registry().with(Bridge { sink });
    tracing::subscriber::with_default(subscriber, || {
        tracing::debug!(target: "rfvp::save", event = "rfvp.save.restore_boundary",
            slot = 4_u64, pending_thread_requests = 2_u64, input_down = false,
            path = "private/source.dat", secret = "do_not_log", payload = ?Private);
        tracing::warn!(target: "core::worker", "commercial dialogue");
        tracing::trace!(target: "core::worker", event = "disabled.event");
    });
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].target, "rfvp::save");
    assert_eq!(events[0].event, "rfvp.save.restore_boundary");
    assert_eq!(events[0].redacted_fields, 3);
    assert!(events[0]
        .fields
        .iter()
        .any(|field| field.name == "slot" && field.value == DiagnosticValue::Unsigned(4)));
    assert_eq!(events[1].event, "family.unstructured_log");
    assert_eq!(events[1].redacted_fields, 1);
    for event in events.iter() {
        event.validate().unwrap();
    }
}

#[test]
fn host_contract_rejects_unbounded_or_ambiguous_diagnostics() {
    let mut event = DiagnosticEvent {
        level: DiagnosticLevel::Info,
        target: "core".into(),
        event: "core.ready".into(),
        fields: Default::default(),
        redacted_fields: 0,
    };
    event.validate().unwrap();
    event.fields.push(DiagnosticField {
        name: "count".into(),
        value: DiagnosticValue::Number(f64::NAN),
    });
    assert!(event.validate().is_err());
    event.fields[0].value = DiagnosticValue::Unsigned(1);
    event.fields.push(event.fields[0].clone());
    assert!(event.validate().is_err());
    event.fields.clear();
    event.target = "private/path".into();
    assert!(event.validate().is_err());
}

#[test]
fn reviewed_audio_kinds_and_script_hashes_remain_bounded_and_typed() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = DiagnosticSink_TO::from_value(Recorder(events.clone()), TD_Opaque);
    let subscriber = tracing_subscriber::registry().with(Bridge { sink });
    let hash = "0123456789abcdef".repeat(4);
    tracing::subscriber::with_default(subscriber, || {
        tracing::error!(
            event = "core.failed",
            slot_kind = "bgm",
            script_hash = hash.as_str()
        );
        tracing::error!(
            event = "core.failed",
            slot_kind = "private_data",
            script_hash = "private/path"
        );
        tracing::error!(event = "core.failed", script_hash = "a".repeat(65).as_str());
        tracing::error!(event = "core.failed", script_hash = "g".repeat(64).as_str());
        tracing::error!(event = "core.failed", script_hash = ?hash);
    });
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 5);
    assert_eq!(events[0].redacted_fields, 0);
    assert!(events[0]
        .fields
        .iter()
        .any(|field| field.name == "slot_kind"
            && field.value == DiagnosticValue::Symbol("bgm".into())));
    assert!(events[0]
        .fields
        .iter()
        .any(|field| field.name == "script_hash"
            && field.value == DiagnosticValue::Symbol(hash.as_str().into())));
    assert_eq!(events[1].redacted_fields, 2);
    for event in &events[2..] {
        assert_eq!(event.redacted_fields, 1);
    }
    for event in events.iter() {
        event.validate().unwrap();
    }
}
