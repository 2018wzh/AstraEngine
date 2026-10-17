//! Process-lifetime C callbacks; Manager owns the typed observability sink.
use astra_emu_abi as abi;
use parking_lot::RwLock;
use std::{
    collections::BTreeMap,
    ffi::c_void,
    sync::{Arc, LazyLock},
};

pub struct NativeDiagnostic {
    pub level: tracing::Level,
    pub target: String,
    pub event: String,
    pub fields: BTreeMap<String, serde_json::Value>,
}
type Emit = Arc<dyn Fn(NativeDiagnostic) + Send + Sync>;
type Flush = Arc<dyn Fn() + Send + Sync>;
type Enabled = Arc<dyn Fn(tracing::Level, &str) -> bool + Send + Sync>;
static EMIT: LazyLock<RwLock<Option<Emit>>> = LazyLock::new(|| RwLock::new(None));
static FLUSH: LazyLock<RwLock<Option<Flush>>> = LazyLock::new(|| RwLock::new(None));
static ENABLED: LazyLock<RwLock<Option<Enabled>>> = LazyLock::new(|| RwLock::new(None));
pub fn set_emit(callback: impl Fn(NativeDiagnostic) + Send + Sync + 'static) {
    *EMIT.write() = Some(Arc::new(callback));
}
pub fn set_flush(callback: impl Fn() + Send + Sync + 'static) {
    *FLUSH.write() = Some(Arc::new(callback));
}
pub fn set_enabled(callback: impl Fn(tracing::Level, &str) -> bool + Send + Sync + 'static) {
    *ENABLED.write() = Some(Arc::new(callback));
}
struct StaticSink(abi::Diagnostics);
// Null context and thread-safe callbacks; immutable for the process lifetime.
unsafe impl Sync for StaticSink {}
static SINK: StaticSink = StaticSink(abi::Diagnostics {
    header: abi::Header::of::<abi::Diagnostics>(),
    context: std::ptr::null_mut(),
    emit: Some(emit),
    flush: Some(flush),
});
pub(crate) fn sink() -> &'static abi::Diagnostics {
    &SINK.0
}
unsafe extern "C" fn flush(_: *mut c_void) {
    let callback = FLUSH.read().clone();
    if let Some(callback) = callback {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback()));
    }
}
unsafe extern "C" fn emit(_: *mut c_void, raw: *const abi::LogRecord) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Some(raw) = (unsafe { raw.as_ref() }) else {
            return;
        };
        let level = match raw.level {
            abi::LOG_ERROR => tracing::Level::ERROR,
            abi::LOG_WARN => tracing::Level::WARN,
            abi::LOG_INFO => tracing::Level::INFO,
            abi::LOG_DEBUG => tracing::Level::DEBUG,
            abi::LOG_TRACE => tracing::Level::TRACE,
            _ => return,
        };
        let callback = EMIT.read().clone();
        let Some(callback) = callback else {
            tracing::error!(
                event = "astra.emu.diagnostics.sink_unavailable",
                code = "ASTRA_EMU_DIAGNOSTIC_SINK_UNAVAILABLE"
            );
            return;
        };
        let Ok(target) = (unsafe { raw.target.text(4096) }) else {
            return;
        };
        let mut dropped = u64::from(raw.dropped_fields);
        let target = if target.is_empty() {
            "astra_emu::core"
        } else {
            bounded_text(target, 256, &mut dropped)
        };
        if ENABLED
            .read()
            .clone()
            .is_some_and(|enabled| !enabled(level, target))
        {
            return;
        }
        let Ok(event) = (unsafe { raw.event.text(4096) }) else {
            return;
        };
        let Ok(message) = (unsafe { raw.message.text(65536) }) else {
            return;
        };
        let event = if event.is_empty() {
            "family.unstructured_log"
        } else {
            bounded_text(event, 256, &mut dropped)
        };
        let message = bounded_text(message, 4096, &mut dropped);
        let Ok(fields) = (unsafe { abi::borrowed_array(raw.fields, raw.field_count, 64) }) else {
            return;
        };
        let mut values = BTreeMap::new();
        for field in fields {
            if field.reserved != 0 || values.len() >= 62 {
                dropped += 1;
                continue;
            }
            let Ok(name) = (unsafe { field.name.text(128) }) else {
                dropped += 1;
                continue;
            };
            if name.is_empty()
                || name == "dropped_fields"
                || (name == "message" && !message.is_empty())
            {
                dropped += 1;
                continue;
            }
            let value = match field.kind {
                abi::FIELD_TEXT => match unsafe { field.text.text(16384) } {
                    Ok(text) => {
                        serde_json::Value::String(bounded_text(text, 4096, &mut dropped).into())
                    }
                    Err(_) => {
                        dropped += 1;
                        continue;
                    }
                },
                abi::FIELD_I64 => field.signed.into(),
                abi::FIELD_U64 => field.unsigned.into(),
                abi::FIELD_F64 => match serde_json::Number::from_f64(field.number) {
                    Some(number) => serde_json::Value::Number(number),
                    None => {
                        dropped += 1;
                        continue;
                    }
                },
                abi::FIELD_BOOL if field.unsigned <= 1 => (field.unsigned != 0).into(),
                _ => {
                    dropped += 1;
                    continue;
                }
            };
            if values.insert(name.to_owned(), value).is_some() {
                dropped += 1;
            }
        }
        if !message.is_empty() {
            values.insert("message".into(), message.into());
        }
        values.insert("dropped_fields".into(), dropped.into());
        callback(NativeDiagnostic {
            level,
            target: target.into(),
            event: event.into(),
            fields: values,
        });
    }));
}
fn bounded_text<'a>(text: &'a str, limit: usize, dropped: &mut u64) -> &'a str {
    if text.len() <= limit {
        return text;
    }
    *dropped += 1;
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
