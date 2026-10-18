//! Bounded native records share the Host filter, writer, ring and critical sink.
use super::*;
use tracing::{
    callsite::{Callsite, Identifier},
    field::FieldSet,
    metadata::Kind,
    Level, Metadata,
};

struct ExternalCallsite;
static CALLSITE: ExternalCallsite = ExternalCallsite;
static METADATA: Metadata<'static> = Metadata::new(
    "external",
    "astra.external",
    Level::INFO,
    None,
    None,
    None,
    FieldSet::new(&[], Identifier(&CALLSITE)),
    Kind::EVENT,
);
impl Callsite for ExternalCallsite {
    // Targets and levels vary per native record; query the retained dispatcher
    // instead of caching one static callsite interest for all external sources.
    fn set_interest(&self, _: tracing::subscriber::Interest) {}
    fn metadata(&self) -> &Metadata<'_> {
        &METADATA
    }
}
fn metadata(level: Level, target: &str) -> Metadata<'_> {
    Metadata::new(
        "external",
        target,
        level,
        None,
        None,
        None,
        FieldSet::new(&[], Identifier(&CALLSITE)),
        Kind::EVENT,
    )
}

impl ObservabilityGuard {
    /// Check the current Host filter before copying external fields. No field
    /// allocation or global/thread-local subscriber lookup is required.
    pub fn external_enabled(&self, level: Level, target: &str) -> bool {
        !target.is_empty()
            && target.len() <= 256
            && !target.contains('\0')
            && self.dispatch.enabled(&metadata(level, target))
    }

    /// Ingest already decoded native fields without Debug stringification or
    /// content redaction. This explicit boundary accepts at most 64 scalar
    /// fields, 128-byte names and 4096-byte strings; target/event <=256 bytes.
    /// Ordinary tracing records retain their existing sanitization policy.
    pub fn record_external(
        &self,
        level: Level,
        target: &str,
        event: &str,
        fields: BTreeMap<String, Value>,
    ) -> Result<(), ObservabilityError> {
        if target.is_empty()
            || target.len() > 256
            || target.contains('\0')
            || event.is_empty()
            || event.len() > 256
            || event.contains('\0')
        {
            return Err(ObservabilityError::InvalidConfig(
                "invalid external record identity",
            ));
        }
        if !self.external_enabled(level, target) {
            return Ok(());
        }
        if fields.len() > 64
            || fields.iter().any(|(name, value)| {
                name.is_empty()
                    || name.len() > 128
                    || name.contains('\0')
                    || match value {
                        Value::String(text) => text.len() > 4096,
                        Value::Array(_) | Value::Object(_) => true,
                        _ => false,
                    }
            })
        {
            return Err(ObservabilityError::InvalidConfig(
                "external record exceeds scalar field bounds",
            ));
        }
        let record = LogEventV1 {
            schema: LOG_EVENT_SCHEMA.to_string(),
            timestamp: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .unwrap_or_else(|_| "timestamp_unavailable".to_string()),
            level: level.as_str().to_string(),
            target: target.to_string(),
            event: event.to_string(),
            session_id: self.session_id.clone(),
            process_role: self.role.as_str().to_string(),
            thread_label: thread::current().name().unwrap_or("unnamed").to_string(),
            span_stack: Vec::new(),
            fields,
        };
        if self.external_console_compact {
            let stderr = std::io::stderr();
            let mut output = stderr.lock();
            let _ = write!(
                output,
                "{} {}: {}",
                record.level, record.target, record.event
            );
            for (name, value) in &record.fields {
                let _ = write!(output, " {name}={value}");
            }
            let _ = writeln!(output);
        }
        self.external.submit(level, &record)
    }
}
