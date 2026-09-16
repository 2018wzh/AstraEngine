//! All dynamic families write through the Manager's existing observability sink.
use abi_stable::sabi_trait::TD_Opaque;
use astra_emu_family_api::{
    DiagnosticEvent, DiagnosticLevel, DiagnosticSink, DiagnosticSinkBox, DiagnosticSink_TO,
};

struct ManagerDiagnostics;

pub(crate) fn sink() -> DiagnosticSinkBox {
    DiagnosticSink_TO::from_value(ManagerDiagnostics, TD_Opaque)
}

impl DiagnosticSink for ManagerDiagnostics {
    fn enabled(&self, level: DiagnosticLevel) -> bool {
        match level {
            DiagnosticLevel::Error => {
                tracing::enabled!(target: "astra_emu::family", tracing::Level::ERROR)
            }
            DiagnosticLevel::Warn => {
                tracing::enabled!(target: "astra_emu::family", tracing::Level::WARN)
            }
            DiagnosticLevel::Info => {
                tracing::enabled!(target: "astra_emu::family", tracing::Level::INFO)
            }
            DiagnosticLevel::Debug => {
                tracing::enabled!(target: "astra_emu::family", tracing::Level::DEBUG)
            }
            DiagnosticLevel::Trace => {
                tracing::enabled!(target: "astra_emu::family", tracing::Level::TRACE)
            }
        }
    }

    fn emit(&self, diagnostic: DiagnosticEvent) {
        if diagnostic.validate().is_err() {
            tracing::error!(target: "astra_emu::family", event = "family.diagnostics.invalid",
                code = "ASTRA_EMU_FAMILY_DIAGNOSTIC");
            return;
        }
        macro_rules! emit {
            ($level:expr) => {
                tracing::event!(target: "astra_emu::family", $level,
                    event = diagnostic.event.as_str(), core_target = diagnostic.target.as_str(),
                    fields = ?diagnostic.fields, redacted_fields = diagnostic.redacted_fields)
            };
        }
        match diagnostic.level {
            DiagnosticLevel::Error => emit!(tracing::Level::ERROR),
            DiagnosticLevel::Warn => emit!(tracing::Level::WARN),
            DiagnosticLevel::Info => emit!(tracing::Level::INFO),
            DiagnosticLevel::Debug => emit!(tracing::Level::DEBUG),
            DiagnosticLevel::Trace => emit!(tracing::Level::TRACE),
        }
    }
}
