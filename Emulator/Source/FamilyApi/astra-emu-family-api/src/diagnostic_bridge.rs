//! Optional adapter-side tracing/log forwarding. This never opens a log file.
//! Install only inside a dynamic library; static cores use the host subscriber.

use std::{fmt, sync::OnceLock};
use tracing::{
    field::{Field, Visit},
    Event, Level, Metadata, Subscriber,
};
use tracing_subscriber::{layer::Context, prelude::*, Layer};

use crate::{
    diagnostic_symbol, DiagnosticEvent, DiagnosticField, DiagnosticLevel, DiagnosticSinkBox,
    DiagnosticValue, FamilyError, FamilyResult, MAX_DIAGNOSTIC_FIELDS,
};

static INSTALLATION: OnceLock<FamilyResult<()>> = OnceLock::new();

/// The first sink is retained until process exit, just like the plugin library.
/// Reopening modules does not add subscribers or duplicate output.
pub fn install(sink: DiagnosticSinkBox) -> FamilyResult<()> {
    let result = INSTALLATION
        .get_or_init(|| {
            tracing_log::LogTracer::init().map_err(|_| initialization_error())?;
            let subscriber = tracing_subscriber::registry().with(Bridge { sink });
            tracing::subscriber::set_global_default(subscriber).map_err(|_| initialization_error())
        })
        .clone();
    if result.is_ok() {
        tracing::info!(target: "astra_emu_family", event = "family.diagnostics.ready");
    }
    result
}

fn initialization_error() -> FamilyError {
    FamilyError::invalid(
        "ASTRA_EMU_FAMILY_DIAGNOSTIC_INIT",
        "family diagnostic subscriber is already owned",
    )
}

struct Bridge {
    sink: DiagnosticSinkBox,
}

fn level(level: &Level) -> DiagnosticLevel {
    match *level {
        Level::ERROR => DiagnosticLevel::Error,
        Level::WARN => DiagnosticLevel::Warn,
        Level::INFO => DiagnosticLevel::Info,
        Level::DEBUG => DiagnosticLevel::Debug,
        Level::TRACE => DiagnosticLevel::Trace,
    }
}

impl<S: Subscriber> Layer<S> for Bridge {
    fn enabled(&self, metadata: &Metadata<'_>, _context: Context<'_, S>) -> bool {
        self.sink.enabled(level(metadata.level()))
    }

    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let metadata = event.metadata();
        let mut visitor = DiagnosticVisitor {
            event: DiagnosticEvent {
                level: level(metadata.level()),
                target: if diagnostic_symbol(metadata.target()) {
                    metadata.target()
                } else {
                    "family"
                }
                .into(),
                event: "family.unstructured_log".into(),
                fields: Default::default(),
                redacted_fields: u32::from(!diagnostic_symbol(metadata.target())),
            },
        };
        if let Some(line) = metadata.line() {
            visitor.push("source_line", DiagnosticValue::Unsigned(line.into()));
        }
        event.record(&mut visitor);
        self.sink.emit(visitor.event);
    }
}

struct DiagnosticVisitor {
    event: DiagnosticEvent,
}

impl DiagnosticVisitor {
    fn redact(&mut self) {
        self.event.redacted_fields = self.event.redacted_fields.saturating_add(1);
    }

    fn push(&mut self, name: &str, value: DiagnosticValue) {
        if self.event.fields.len() == MAX_DIAGNOSTIC_FIELDS
            || !diagnostic_symbol(name)
            || self.event.fields.iter().any(|field| field.name == name)
        {
            self.redact();
            return;
        }
        self.event.fields.push(DiagnosticField {
            name: name.into(),
            value,
        });
    }
}

impl Visit for DiagnosticVisitor {
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.push(field.name(), DiagnosticValue::Signed(value));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.push(field.name(), DiagnosticValue::Unsigned(value));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.push(field.name(), DiagnosticValue::Bool(value));
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        if value.is_finite() {
            self.push(field.name(), DiagnosticValue::Number(value));
        } else {
            self.redact();
        }
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        // Lexical bounds are not a privacy policy. Only reviewed symbolic fields
        // may carry strings; paths, dialogue, messages and Debug never cross.
        match field.name() {
            "event" if diagnostic_symbol(value) => self.event.event = value.into(),
            "log.target" if diagnostic_symbol(value) => self.event.target = value.into(),
            "code" | "operation" | "state" if diagnostic_symbol(value) => {
                self.push(field.name(), DiagnosticValue::Symbol(value.into()));
            }
            "slot_kind" if matches!(value, "bgm" | "se" | "none") => {
                self.push(field.name(), DiagnosticValue::Symbol(value.into()));
            }
            "script_hash"
                if value.len() == 64
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) =>
            {
                self.push(field.name(), DiagnosticValue::Symbol(value.into()));
            }
            _ => self.redact(),
        }
    }
    fn record_debug(&mut self, _field: &Field, _value: &dyn fmt::Debug) {
        self.redact();
    }
}

#[cfg(test)]
mod tests;
