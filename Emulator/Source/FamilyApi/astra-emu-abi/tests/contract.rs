use astra_emu_abi::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

fn integer(value: i64) -> ConfigValue {
    ConfigValue {
        kind: VALUE_INTEGER,
        reserved: 0,
        integer: value,
        number: 0.0,
        text: Bytes::EMPTY,
    }
}
fn field() -> ConfigField {
    ConfigField {
        id: Bytes::new(b"render_width"),
        label: Bytes::new(b"Render width"),
        group: Bytes::EMPTY,
        default_value: integer(1280),
        integer_min: 320,
        integer_max: 7680,
        number_min: 0.0,
        number_max: 0.0,
        max_bytes: 0,
        reserved: 0,
        choices: std::ptr::null(),
        choice_count: 0,
    }
}
#[test]
fn configuration_rejects_unknown_duplicate_and_out_of_range_values() {
    let fields = [field()];
    let entry = ConfigEntry {
        id: Bytes::new(b"render_width"),
        value: integer(1920),
    };
    unsafe {
        validate_config(&fields, &[]).unwrap();
        validate_config(&fields, &[entry]).unwrap();
        assert_eq!(
            validate_config(&fields, &[entry, entry]).unwrap_err().index,
            1
        );
        assert_eq!(
            validate_config(
                &fields,
                &[ConfigEntry {
                    id: Bytes::new(b"unknown"),
                    ..entry
                }]
            )
            .unwrap_err()
            .reason,
            "unknown value ID"
        );
        assert_eq!(
            validate_config(
                &fields,
                &[ConfigEntry {
                    value: integer(319),
                    ..entry
                }]
            )
            .unwrap_err()
            .reason,
            "value type or range mismatch"
        );
        let wrong_type = ConfigValue {
            kind: VALUE_NUMBER,
            number: 1920.0,
            ..integer(0)
        };
        assert!(validate_config(
            &fields,
            &[ConfigEntry {
                value: wrong_type,
                ..entry
            }]
        )
        .is_err());
        let invalid_default = ConfigField {
            default_value: integer(7681),
            ..field()
        };
        assert!(validate_config(&[invalid_default], &[]).is_err());
    }
}
#[test]
fn schema_rejects_nonfinite_ranges_and_duplicate_enum_choices() {
    let number = ConfigField {
        default_value: ConfigValue {
            kind: VALUE_NUMBER,
            number: 1.0,
            ..integer(0)
        },
        number_min: 0.0,
        number_max: f64::INFINITY,
        ..field()
    };
    let choices = [Bytes::new(b"native"), Bytes::new(b"native")];
    let enumeration = ConfigField {
        default_value: ConfigValue {
            kind: VALUE_ENUM,
            text: Bytes::new(b"native"),
            ..integer(0)
        },
        choices: choices.as_ptr(),
        choice_count: choices.len(),
        ..field()
    };
    unsafe {
        assert!(validate_config(&[number], &[]).is_err());
        assert!(validate_config(&[enumeration], &[]).is_err());
    }
}
struct Storage {
    bytes: [u8; 16],
    drops: Arc<AtomicUsize>,
}
impl Drop for Storage {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
unsafe extern "C" fn release(owner: *mut std::ffi::c_void) {
    drop(unsafe { Box::from_raw(owner.cast::<Storage>()) });
}
fn frame(drops: Arc<AtomicUsize>) -> FrameSurface {
    let storage = Box::new(Storage {
        bytes: [73; 16],
        drops,
    });
    let pixels = Bytes::new(&storage.bytes);
    FrameSurface {
        state: FRAME_READY,
        reserved: 0,
        identity: FrameIdentity {
            session_epoch: 3,
            output_epoch: 7,
            generation: 11,
        },
        timestamp_ns: 900,
        info: SurfaceInfo {
            kind: SURFACE_CPU,
            pixel_format: PIXEL_RGBA8,
            color: COLOR_SRGB,
            alpha: ALPHA_IGNORED,
            rows: ROWS_TOP_DOWN,
            backend: 0,
            logical: Extent {
                width: 1280,
                height: 720,
            },
            raster: Extent {
                width: 2,
                height: 2,
            },
            stride: 8,
        },
        cpu: CpuLease {
            pixels,
            owner: Box::into_raw(storage).cast(),
            release: Some(release),
        },
    }
}
#[test]
fn cpu_lease_preserves_allocation_until_last_consumer_releases_on_other_thread() {
    let drops = Arc::new(AtomicUsize::new(0));
    let raw = frame(drops.clone());
    let allocation = raw.cpu.pixels.data as usize;
    let owner = Arc::new(unsafe { CpuFrame::adopt(raw, 3) }.unwrap());
    let consumer = owner.clone();
    drop(owner);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    std::thread::spawn(move || {
        assert_eq!(consumer.pixels(), &[73; 16]);
        assert_eq!(consumer.pixels().as_ptr() as usize, allocation);
        assert_eq!(consumer.identity().output_epoch, 7);
    })
    .join()
    .unwrap();
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
#[test]
fn rejected_epoch_or_stride_returns_the_owned_lease_once() {
    let drops = Arc::new(AtomicUsize::new(0));
    assert_eq!(
        unsafe { CpuFrame::adopt(frame(drops.clone()), 4) }.unwrap_err(),
        STALE_CONTEXT
    );
    let mut invalid = frame(drops.clone());
    invalid.info.stride = 4;
    assert_eq!(
        unsafe { CpuFrame::adopt(invalid, 3) }.unwrap_err(),
        INVALID_ARGUMENT
    );
    assert_eq!(drops.load(Ordering::SeqCst), 2);
}
#[test]
fn spans_reject_null_nonempty_and_invalid_utf8_without_dereferencing_null() {
    unsafe {
        assert_eq!(
            Bytes {
                data: std::ptr::null(),
                len: 1
            }
            .as_slice(10),
            Err(INVALID_ARGUMENT)
        );
        assert_eq!(Bytes::EMPTY.as_slice(10), Ok(&[][..]));
        assert_eq!(Bytes::new(&[0xff]).text(10), Err(INVALID_ARGUMENT));
        assert_eq!(Bytes::new(b"a\0b").text(10), Err(INVALID_ARGUMENT));
    }
}

#[test]
fn retry_requires_a_retained_failure_and_bounded_diagnostic() {
    let mut state = SessionState {
        state: SESSION_FAILED,
        flags: SESSION_CAN_RETRY,
        context_epoch: 1,
        output_epoch: 1,
        error: Error::default(),
    };
    assert_eq!(state.validate(), Err(INVALID_ARGUMENT));
    state
        .error
        .set("CORE_HOOK_FAILED", "retry-safe native operation");
    state.validate().unwrap();
    state.state = SESSION_RUNNING;
    assert_eq!(state.validate(), Err(INVALID_ARGUMENT));
    state.state = SESSION_FAILED;
    state.flags = 2;
    assert_eq!(state.validate(), Err(INVALID_ARGUMENT));
    state.flags = 0;
    state.validate().unwrap();
    state.error.code_len = 129;
    assert_eq!(state.validate(), Err(INVALID_ARGUMENT));
}
