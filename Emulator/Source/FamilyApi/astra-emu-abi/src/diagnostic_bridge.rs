//! Dynamic adapter tracing/log bridge. Static cores use the Host subscriber.
use crate::*;
use std::{fmt, mem::MaybeUninit, sync::OnceLock};
use tracing::{
    field::{Field, Visit},
    Event, Level, Subscriber,
};
use tracing_subscriber::{layer::Context, prelude::*, Layer};

const MAX_FIELDS: usize = 32;
const MAX_TEXT: usize = 4096;
const TEXT_BUDGET: usize = 16384;
static INSTALLATION: OnceLock<Result<Sink, Status>> = OnceLock::new();
#[derive(Clone, Copy)]
struct Sink(Diagnostics);
// SAFETY: install requires a process-lifetime, thread-safe host callback object.
unsafe impl Send for Sink {}
unsafe impl Sync for Sink {}

/// Installs exactly one sink per dynamic module. A different subsequent sink is
/// a conflict, not a silent redirection. Failure remains sticky.
/// # Safety
/// Host callbacks/context must remain valid until process exit and be callable
/// concurrently from arbitrary threads without unwinding across the ABI.
pub unsafe fn install(sink: Diagnostics) -> Result<(), Status> {
    if !sink.header.accepts::<Diagnostics>() || sink.emit.is_none() || sink.flush.is_none() {
        return Err(INVALID_ARGUMENT);
    }
    let installed = INSTALLATION.get_or_init(|| {
        tracing_log::LogTracer::init().map_err(|_| INVALID_STATE)?;
        let subscriber = tracing_subscriber::registry().with(Bridge(Sink(sink)));
        tracing::subscriber::set_global_default(subscriber).map_err(|_| INVALID_STATE)?;
        Ok(Sink(sink))
    });
    match installed {
        Ok(previous)
            if previous.0.context == sink.context
                && std::ptr::fn_addr_eq(previous.0.emit.unwrap(), sink.emit.unwrap())
                && std::ptr::fn_addr_eq(previous.0.flush.unwrap(), sink.flush.unwrap()) =>
        {
            Ok(())
        }
        Ok(_) => Err(INVALID_STATE),
        Err(status) => Err(*status),
    }
}
struct Bridge(Sink);
fn level(value: &Level) -> u32 {
    match *value {
        Level::ERROR => LOG_ERROR,
        Level::WARN => LOG_WARN,
        Level::INFO => LOG_INFO,
        Level::DEBUG => LOG_DEBUG,
        Level::TRACE => LOG_TRACE,
    }
}
impl<S: Subscriber> Layer<S> for Bridge {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let mut visitor = Visitor::new();
        if let Some(line) = event.metadata().line() {
            visitor.push("source_line", Value::Unsigned(line.into()));
        }
        event.record(&mut visitor);
        let mut fields = [LogField {
            name: Bytes::EMPTY,
            kind: 0,
            reserved: 0,
            text: Bytes::EMPTY,
            signed: 0,
            unsigned: 0,
            number: 0.0,
        }; MAX_FIELDS];
        for (output, input) in fields.iter_mut().zip(&visitor.fields[..visitor.count]) {
            output.name = Bytes::new(input.name.as_bytes());
            match input.value {
                Value::Text(range) => {
                    output.kind = FIELD_TEXT;
                    output.text = visitor.span(range);
                }
                Value::Signed(value) => {
                    output.kind = FIELD_I64;
                    output.signed = value;
                }
                Value::Unsigned(value) => {
                    output.kind = FIELD_U64;
                    output.unsigned = value;
                }
                Value::Number(value) => {
                    output.kind = FIELD_F64;
                    output.number = value;
                }
                Value::Bool(value) => {
                    output.kind = FIELD_BOOL;
                    output.unsigned = u64::from(value);
                }
            }
        }
        let record = LogRecord {
            level: level(event.metadata().level()),
            dropped_fields: visitor.dropped,
            target: visitor
                .target
                .map(|r| visitor.span(r))
                .unwrap_or_else(|| Bytes::new(event.metadata().target().as_bytes())),
            event: visitor
                .event
                .map(|r| visitor.span(r))
                .unwrap_or_else(|| Bytes::new(b"family.unstructured_log")),
            message: visitor
                .message
                .map(|r| visitor.span(r))
                .unwrap_or(Bytes::EMPTY),
            fields: fields.as_ptr(),
            field_count: visitor.count,
        };
        // SAFETY: initialized process-lifetime callback; every span lives for call.
        unsafe { self.0 .0.emit.unwrap()(self.0 .0.context, &record) };
    }
}
#[derive(Clone, Copy)]
struct Range {
    start: usize,
    len: usize,
}
#[derive(Clone, Copy)]
enum Value {
    Text(Range),
    Signed(i64),
    Unsigned(u64),
    Number(f64),
    Bool(bool),
}
#[derive(Clone, Copy)]
struct OwnedField {
    name: &'static str,
    value: Value,
}
struct Visitor {
    fields: [OwnedField; MAX_FIELDS],
    count: usize,
    dropped: u32,
    bytes: [MaybeUninit<u8>; TEXT_BUDGET],
    used: usize,
    event: Option<Range>,
    target: Option<Range>,
    message: Option<Range>,
}
impl Visitor {
    fn new() -> Self {
        Self {
            fields: [OwnedField {
                name: "",
                value: Value::Unsigned(0),
            }; MAX_FIELDS],
            count: 0,
            dropped: 0,
            bytes: [MaybeUninit::uninit(); TEXT_BUDGET],
            used: 0,
            event: None,
            target: None,
            message: None,
        }
    }
    fn drop_field(&mut self) {
        self.dropped = self.dropped.saturating_add(1);
    }
    fn push(&mut self, name: &'static str, value: Value) {
        if self.count == MAX_FIELDS
            || name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || self.fields[..self.count].iter().any(|f| f.name == name)
        {
            self.drop_field();
            return;
        }
        self.fields[self.count] = OwnedField { name, value };
        self.count += 1;
    }
    fn store(&mut self, text: &str) -> Option<Range> {
        if text.len() > MAX_TEXT || text.len() > TEXT_BUDGET - self.used {
            self.drop_field();
            return None;
        }
        let range = Range {
            start: self.used,
            len: text.len(),
        };
        for (output, byte) in self.bytes[self.used..].iter_mut().zip(text.bytes()) {
            output.write(byte);
        }
        self.used += text.len();
        Some(range)
    }
    fn span(&self, range: Range) -> Bytes {
        // store/BoundedText initialize every byte in a published range.
        Bytes {
            data: self.bytes[range.start..].as_ptr().cast(),
            len: range.len,
        }
    }
    fn text(&mut self, name: &'static str, range: Range) {
        match name {
            "event" if range.len != 0 => self.event = Some(range),
            "log.target" if range.len != 0 => self.target = Some(range),
            "message" => self.message = Some(range),
            _ => self.push(name, Value::Text(range)),
        }
    }
}
impl Visit for Visitor {
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.push(field.name(), Value::Signed(value));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.push(field.name(), Value::Unsigned(value));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.push(field.name(), Value::Bool(value));
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        if value.is_finite() {
            self.push(field.name(), Value::Number(value));
        } else {
            self.drop_field();
        }
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        if let Some(range) = self.store(value) {
            self.text(field.name(), range);
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        use fmt::Write;
        let start = self.used;
        let mut output = BoundedText {
            buffer: &mut self.bytes[start..],
            used: 0,
        };
        if write!(&mut output, "{value:?}").is_err() {
            self.drop_field();
            return;
        }
        let len = output.used;
        self.used += len;
        self.text(field.name(), Range { start, len });
    }
}
struct BoundedText<'a> {
    buffer: &'a mut [MaybeUninit<u8>],
    used: usize,
}
impl fmt::Write for BoundedText<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let end = self.used.checked_add(value.len()).ok_or(fmt::Error)?;
        if end > self.buffer.len() || end > MAX_TEXT {
            return Err(fmt::Error);
        }
        for (output, byte) in self.buffer[self.used..end].iter_mut().zip(value.bytes()) {
            output.write(byte);
        }
        self.used = end;
        Ok(())
    }
}
