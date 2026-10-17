#![cfg(feature = "diagnostic-bridge")]
use astra_emu_abi::*;
use parking_lot::Mutex;
use std::{fmt, sync::LazyLock};
#[derive(Debug)]
struct Record {
    event: String,
    message: String,
    fields: Vec<(String, u32, i64, u64, f64)>,
    dropped: u32,
}
static RECORDS: LazyLock<Mutex<Vec<Record>>> = LazyLock::new(|| Mutex::new(Vec::new()));
unsafe extern "C" fn emit(_: *mut std::ffi::c_void, raw: *const LogRecord) {
    let record = unsafe { &*raw };
    let fields = unsafe { borrowed_array(record.fields, record.field_count, 32) }.unwrap();
    RECORDS.lock().push(Record {
        event: unsafe { record.event.text(4096) }.unwrap().into(),
        message: unsafe { record.message.text(4096) }.unwrap().into(),
        fields: fields
            .iter()
            .map(|f| {
                (
                    unsafe { f.name.text(128) }.unwrap().into(),
                    f.kind,
                    f.signed,
                    f.unsigned,
                    f.number,
                )
            })
            .collect(),
        dropped: record.dropped_fields,
    });
}
unsafe extern "C" fn flush(_: *mut std::ffi::c_void) {}
struct Oversized;
impl fmt::Debug for Oversized {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        for _ in 0..5000 {
            out.write_str("x")?;
        }
        Ok(())
    }
}
#[test]
fn diagnostics_preserve_typed_values_bound_debug_and_reject_sink_replacement() {
    let sink = Diagnostics {
        header: Header::of::<Diagnostics>(),
        context: std::ptr::null_mut(),
        emit: Some(emit),
        flush: Some(flush),
    };
    unsafe {
        diagnostic_bridge::install(sink).unwrap();
        diagnostic_bridge::install(sink).unwrap();
    }
    tracing::info!(event = "native.diagnostic", number = 7u64, signed = -2i64, real = 0.5f64,
        truth = true, message = "diagnostic text", oversized = ?Oversized);
    let records = RECORDS.lock();
    let record = records
        .iter()
        .find(|r| r.event == "native.diagnostic")
        .unwrap();
    assert_eq!(record.message, "diagnostic text");
    assert_eq!(record.dropped, 1);
    assert!(record
        .fields
        .contains(&("number".into(), FIELD_U64, 0, 7, 0.0)));
    assert!(record
        .fields
        .contains(&("signed".into(), FIELD_I64, -2, 0, 0.0)));
    assert!(record
        .fields
        .contains(&("real".into(), FIELD_F64, 0, 0, 0.5)));
    assert!(record
        .fields
        .contains(&("truth".into(), FIELD_BOOL, 0, 1, 0.0)));
    static ANOTHER_SINK: u8 = 0;
    let replaced = Diagnostics {
        context: (&ANOTHER_SINK as *const u8).cast_mut().cast(),
        ..sink
    };
    assert_eq!(
        unsafe { diagnostic_bridge::install(replaced) },
        Err(INVALID_STATE)
    );
}
