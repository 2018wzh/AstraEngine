#![cfg(feature = "diagnostic-bridge")]

use abi_stable::sabi_trait::TD_Opaque;
use astra_emu_family_api::{
    diagnostic_bridge, DiagnosticEvent, DiagnosticLevel, DiagnosticSink, DiagnosticSink_TO,
};
use std::sync::{Arc, Mutex};

struct Recorder(Arc<Mutex<Vec<DiagnosticEvent>>>);
impl DiagnosticSink for Recorder {
    fn enabled(&self, _: DiagnosticLevel) -> bool {
        true
    }
    fn emit(&self, event: DiagnosticEvent) {
        self.0.lock().unwrap().push(event);
    }
}

#[test]
fn worker_logs_and_repeated_install_use_one_process_sink() {
    let events = Arc::new(Mutex::new(Vec::new()));
    for _ in 0..3 {
        diagnostic_bridge::install(DiagnosticSink_TO::from_value(
            Recorder(events.clone()),
            TD_Opaque,
        ))
        .unwrap();
    }
    std::thread::spawn(|| {
        tracing::debug!(target: "core::save", event = "core.save.restore", slot = 4_u64);
        log::warn!(target: "core::audio", "unreviewed private payload");
    })
    .join()
    .unwrap();
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 5);
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event == "family.diagnostics.ready")
            .count(),
        3
    );
    assert_eq!(events[3].event, "core.save.restore");
    assert_eq!(events[4].target, "core::audio");
    assert_eq!(events[4].level, DiagnosticLevel::Warn);
    assert!(events[4].redacted_fields > 0);
    for event in events.iter() {
        event.validate().unwrap();
    }
}
