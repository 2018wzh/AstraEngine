//! Low-volume startup phase markers for the existing product diagnostic sink.
use std::{sync::OnceLock, time::Instant};

static STARTED: OnceLock<Instant> = OnceLock::new();

/// Start at the application entry, before initializing the diagnostic sink.
pub fn begin() {
    let _ = STARTED.set(Instant::now());
}

/// Records a fixed phase name without paths, user data, or a new logging sink.
pub fn mark(phase: &'static str) {
    let elapsed = STARTED.get_or_init(Instant::now).elapsed();
    tracing::info!(
        event = "astra.emu.startup.phase",
        phase,
        elapsed_ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64,
    );
}
