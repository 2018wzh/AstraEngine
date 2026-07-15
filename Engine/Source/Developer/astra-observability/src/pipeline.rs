use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
};

use file_rotate::{compression::Compression, suffix::AppendCount, ContentLimit, FileRotate};
use serde_json::Value;
use thiserror::Error;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tracing::{field::Visit, Event, Id, Subscriber};
use tracing_subscriber::{
    layer::{Context, SubscriberExt},
    registry::{LookupSpan, Registry},
    reload, EnvFilter, Layer,
};

use crate::{
    config::{ConsoleFormat, HostObservabilityConfig},
    crash::{write_crash_bundle, CrashBundleManifestV1},
    record::{sanitize_field, LogEventV1, SpanContextV1, LOG_EVENT_SCHEMA},
    ring::RingBuffer,
};

#[derive(Debug, Error)]
pub enum ObservabilityError {
    #[error("invalid observability configuration: {0}")]
    InvalidConfig(&'static str),
    #[error("invalid log filter: {0}")]
    Filter(#[from] tracing_subscriber::filter::ParseError),
    #[error("install tracing subscriber: {0}")]
    Install(#[from] tracing::subscriber::SetGlobalDefaultError),
    #[error("reload log filter: {0}")]
    Reload(String),
    #[error("observability I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("observability JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("observability writer stopped")]
    WriterStopped,
    #[error("crash reporter: {0}")]
    CrashReporter(String),
}

pub struct ObservabilityGuard {
    session_id: String,
    role: crate::HostRole,
    reload: reload::Handle<EnvFilter, Registry>,
    writer: SyncSender<WriterCommand>,
    writer_thread: Mutex<Option<JoinHandle<()>>>,
    ring: Arc<Mutex<RingBuffer>>,
    dropped: Arc<AtomicU64>,
    crash_dir: Option<std::path::PathBuf>,
    max_crash_bundles: usize,
}

impl ObservabilityGuard {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn reload_filter(&self, filter: &str) -> Result<(), ObservabilityError> {
        self.reload
            .reload(EnvFilter::try_new(filter)?)
            .map_err(|error| ObservabilityError::Reload(error.to_string()))
    }

    pub fn flush(&self) -> Result<(), ObservabilityError> {
        let (complete, wait) = mpsc::sync_channel(0);
        self.writer
            .send(WriterCommand::Flush(complete))
            .map_err(|_| ObservabilityError::WriterStopped)?;
        wait.recv().map_err(|_| ObservabilityError::WriterStopped)
    }

    pub fn write_fatal_bundle(
        &self,
        reason_code: &str,
    ) -> Result<CrashBundleManifestV1, ObservabilityError> {
        self.flush()?;
        let root = self
            .crash_dir
            .as_ref()
            .ok_or(ObservabilityError::InvalidConfig(
                "fatal bundle needs a crash directory",
            ))?;
        let ring = self
            .ring
            .lock()
            .map_err(|_| ObservabilityError::WriterStopped)?;
        write_crash_bundle(
            root,
            reason_code,
            &self.session_id,
            self.role,
            &ring,
            self.dropped.load(Ordering::Relaxed),
            self.max_crash_bundles,
        )
    }
}

impl Drop for ObservabilityGuard {
    fn drop(&mut self) {
        let _ = self.flush();
        let _ = self.writer.send(WriterCommand::Shutdown);
        if let Ok(slot) = self.writer_thread.get_mut() {
            if let Some(handle) = slot.take() {
                let _ = handle.join();
            }
        }
    }
}

pub fn init_host(
    config: HostObservabilityConfig,
) -> Result<ObservabilityGuard, ObservabilityError> {
    config
        .validate()
        .map_err(ObservabilityError::InvalidConfig)?;
    if let Some(directory) = &config.log_dir {
        fs::create_dir_all(directory)?;
    }
    let session_id = uuid::Uuid::new_v4().to_string();
    let ring = Arc::new(Mutex::new(RingBuffer::new(
        config.ring_max_records,
        config.ring_max_bytes,
    )));
    let dropped = Arc::new(AtomicU64::new(0));
    let (writer, receiver) = mpsc::sync_channel(1024);
    let writer_thread = spawn_writer(&config, receiver)?;
    let critical = config.log_dir.as_ref().map(|directory| {
        Arc::new(Mutex::new(rotating_file(
            directory.join("astra-critical.jsonl"),
            config.max_file_bytes,
            config.max_archives,
        )))
    });
    let stable = StableJsonLayer {
        session_id: session_id.clone(),
        role: config.role.as_str().to_string(),
        ring: Arc::clone(&ring),
        writer: writer.clone(),
        critical,
        dropped: Arc::clone(&dropped),
        console_json: config.console && config.console_format == ConsoleFormat::Json,
    };
    let filter = EnvFilter::try_new(&config.filter)?;
    let (filter_layer, reload) = reload::Layer::new(filter);

    match (config.console, config.console_format) {
        (true, ConsoleFormat::Compact) => install_subscriber(
            filter_layer,
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .compact()
                .with_writer(std::io::stderr),
            stable,
        )?,
        (true, ConsoleFormat::Json) => install_subscriber(
            filter_layer,
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .compact()
                .with_writer(std::io::sink),
            stable,
        )?,
        (false, _) => install_subscriber(
            filter_layer,
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .compact()
                .with_writer(std::io::sink),
            stable,
        )?,
    }

    Ok(ObservabilityGuard {
        session_id,
        role: config.role,
        reload,
        writer,
        writer_thread: Mutex::new(Some(writer_thread)),
        ring,
        dropped,
        crash_dir: config.crash_dir,
        max_crash_bundles: config.max_crash_bundles,
    })
}

fn install_subscriber<L>(
    filter: reload::Layer<EnvFilter, Registry>,
    console: L,
    stable: StableJsonLayer,
) -> Result<(), tracing::subscriber::SetGlobalDefaultError>
where
    L: Layer<tracing_subscriber::layer::Layered<reload::Layer<EnvFilter, Registry>, Registry>>
        + Send
        + Sync
        + 'static,
{
    let subscriber = Registry::default().with(filter).with(console).with(stable);
    tracing::subscriber::set_global_default(subscriber)
}

#[derive(Clone)]
struct StableJsonLayer {
    session_id: String,
    role: String,
    ring: Arc<Mutex<RingBuffer>>,
    writer: SyncSender<WriterCommand>,
    critical: Option<Arc<Mutex<FileRotate<AppendCount>>>>,
    dropped: Arc<AtomicU64>,
    console_json: bool,
}

#[derive(Debug, Clone, Default)]
struct RecordedFields(BTreeMap<String, Value>);

impl<S> Layer<S> for StableJsonLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(
        &self,
        attributes: &tracing::span::Attributes<'_>,
        id: &Id,
        ctx: Context<'_, S>,
    ) {
        if let Some(span) = ctx.span(id) {
            let mut fields = FieldVisitor::default();
            attributes.record(&mut fields);
            span.extensions_mut().insert(RecordedFields(fields.fields));
        }
    }

    fn on_record(&self, id: &Id, values: &tracing::span::Record<'_>, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let mut extensions = span.extensions_mut();
            let fields = extensions.get_mut::<RecordedFields>();
            if let Some(fields) = fields {
                let mut visitor = FieldVisitor::default();
                values.record(&mut visitor);
                fields.0.extend(visitor.fields);
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        let event_name = visitor
            .fields
            .remove("event")
            .or_else(|| visitor.fields.remove("message"))
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| metadata.name().to_string());
        let span_stack = ctx
            .event_scope(event)
            .map(|scope| {
                scope
                    .from_root()
                    .map(|span| SpanContextV1 {
                        name: span.metadata().name().to_string(),
                        target: span.metadata().target().to_string(),
                        fields: span
                            .extensions()
                            .get::<RecordedFields>()
                            .map(|fields| fields.0.clone())
                            .unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let record = LogEventV1 {
            schema: LOG_EVENT_SCHEMA.to_string(),
            timestamp: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .unwrap_or_else(|_| "timestamp_unavailable".to_string()),
            level: metadata.level().as_str().to_string(),
            target: metadata.target().to_string(),
            event: event_name,
            session_id: self.session_id.clone(),
            process_role: self.role.clone(),
            thread_label: thread::current().name().unwrap_or("unnamed").to_string(),
            span_stack,
            fields: visitor.fields,
        };
        let Ok(mut encoded) = serde_json::to_string(&record) else {
            return;
        };
        encoded.push('\n');
        if self.console_json {
            let _ = std::io::stderr().write_all(encoded.as_bytes());
        }
        if let Ok(mut ring) = self.ring.lock() {
            ring.push(encoded.trim_end().to_string());
        }
        if matches!(
            *metadata.level(),
            tracing::Level::WARN | tracing::Level::ERROR
        ) {
            if let Some(writer) = &self.critical {
                if let Ok(mut writer) = writer.lock() {
                    let _ = writer.write_all(encoded.as_bytes());
                    let _ = writer.flush();
                }
            }
        }
        match self.writer.try_send(WriterCommand::Record(encoded)) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                let dropped = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                if dropped == 1 || dropped.is_multiple_of(1024) {
                    self.record_drop_warning(dropped);
                }
            }
            Err(TrySendError::Disconnected(_)) => {
                let dropped = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                self.record_drop_warning(dropped);
            }
        }
    }
}

impl StableJsonLayer {
    fn record_drop_warning(&self, dropped_count: u64) {
        let mut fields = BTreeMap::new();
        fields.insert("dropped_count".to_string(), Value::from(dropped_count));
        fields.insert(
            "diagnostic_code".to_string(),
            Value::String("ASTRA_LOG_QUEUE_SATURATED".to_string()),
        );
        let warning = LogEventV1 {
            schema: LOG_EVENT_SCHEMA.to_string(),
            timestamp: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .unwrap_or_else(|_| "timestamp_unavailable".to_string()),
            level: "WARN".to_string(),
            target: "astra_observability".to_string(),
            event: "observability.queue.saturated".to_string(),
            session_id: self.session_id.clone(),
            process_role: self.role.clone(),
            thread_label: thread::current().name().unwrap_or("unnamed").to_string(),
            span_stack: Vec::new(),
            fields,
        };
        let Ok(mut encoded) = serde_json::to_string(&warning) else {
            return;
        };
        encoded.push('\n');
        if let Ok(mut ring) = self.ring.lock() {
            ring.push(encoded.trim_end().to_string());
        }
        if let Some(writer) = &self.critical {
            if let Ok(mut writer) = writer.lock() {
                let _ = writer.write_all(encoded.as_bytes());
                let _ = writer.flush();
            }
        }
        if self.console_json {
            let _ = std::io::stderr().write_all(encoded.as_bytes());
        }
    }
}

#[derive(Default)]
struct FieldVisitor {
    fields: BTreeMap<String, Value>,
}

impl FieldVisitor {
    fn insert(&mut self, field: &tracing::field::Field, value: Value) {
        self.fields.insert(
            field.name().to_string(),
            sanitize_field(field.name(), value),
        );
    }
}

impl Visit for FieldVisitor {
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.insert(field, Value::Bool(value));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.insert(field, Value::from(value));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.insert(field, Value::from(value));
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.insert(field, Value::from(value));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.insert(field, Value::String(value.to_string()));
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.insert(field, Value::String(format!("{value:?}")));
    }

    fn record_error(
        &mut self,
        field: &tracing::field::Field,
        _value: &(dyn std::error::Error + 'static),
    ) {
        self.insert(field, Value::String("[error redacted]".to_string()));
    }
}

enum WriterCommand {
    Record(String),
    Flush(SyncSender<()>),
    Shutdown,
}

fn spawn_writer(
    config: &HostObservabilityConfig,
    receiver: Receiver<WriterCommand>,
) -> Result<JoinHandle<()>, std::io::Error> {
    let mut writer = config.log_dir.as_ref().map(|directory| {
        rotating_file(
            directory.join("astra.jsonl"),
            config.max_file_bytes,
            config.max_archives,
        )
    });
    thread::Builder::new()
        .name("astra-log-writer".to_string())
        .spawn(move || {
            while let Ok(command) = receiver.recv() {
                match command {
                    WriterCommand::Record(record) => {
                        if let Some(writer) = writer.as_mut() {
                            let _ = writer.write_all(record.as_bytes());
                        }
                    }
                    WriterCommand::Flush(complete) => {
                        if let Some(writer) = writer.as_mut() {
                            let _ = writer.flush();
                        }
                        let _ = complete.send(());
                    }
                    WriterCommand::Shutdown => {
                        if let Some(writer) = writer.as_mut() {
                            let _ = writer.flush();
                        }
                        break;
                    }
                }
            }
        })
}

fn rotating_file(
    path: std::path::PathBuf,
    bytes: usize,
    archives: usize,
) -> FileRotate<AppendCount> {
    FileRotate::new(
        path,
        AppendCount::new(archives),
        ContentLimit::BytesSurpassed(bytes),
        Compression::None,
        None,
    )
}
