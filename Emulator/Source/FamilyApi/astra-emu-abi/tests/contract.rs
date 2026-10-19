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
        flags: 0,
        choices: std::ptr::null(),
        choice_count: 0,
        description: Bytes::EMPTY,
        choice_labels: std::ptr::null(),
        step: 0.0,
        unit: Bytes::EMPTY,
    }
}
fn text(value: &'static [u8]) -> ConfigValue {
    ConfigValue {
        kind: VALUE_STRING,
        text: Bytes::from_static(value),
        ..integer(0)
    }
}
fn path_field(flags: u32) -> ConfigField {
    ConfigField {
        id: Bytes::new(b"archive"),
        default_value: text(b""),
        max_bytes: 260,
        flags,
        ..field()
    }
}
const SCHEMA_ERROR: &str = "invalid schema, duplicate ID or invalid default";
fn schema_error(field: ConfigField) -> Option<&'static str> {
    unsafe { validate_config(&[field], &[]) }
        .err()
        .map(|e| e.reason)
}
#[test]
fn enum_choice_labels_must_match_choice_count() {
    let choices = [Bytes::new(b"shift_jis"), Bytes::new(b"gbk")];
    let labels = [
        Bytes::new("Shift-JIS（日文原版）".as_bytes()),
        Bytes::new("GBK（简体中文）".as_bytes()),
    ];
    let enumeration = ConfigField {
        id: Bytes::new(b"script_encoding"),
        default_value: ConfigValue {
            kind: VALUE_ENUM,
            text: Bytes::new(b"shift_jis"),
            ..integer(0)
        },
        choices: choices.as_ptr(),
        choice_count: choices.len(),
        choice_labels: labels.as_ptr(),
        ..field()
    };
    let empty_label = [Bytes::new(b"Shift-JIS"), Bytes::EMPTY];
    assert_eq!(schema_error(enumeration), None);
    assert_eq!(
        schema_error(ConfigField {
            choice_labels: std::ptr::null(),
            ..enumeration
        }),
        Some(SCHEMA_ERROR)
    );
    assert_eq!(
        schema_error(ConfigField {
            choice_labels: empty_label.as_ptr(),
            ..enumeration
        }),
        Some(SCHEMA_ERROR)
    );
    assert_eq!(
        schema_error(ConfigField {
            choice_labels: labels.as_ptr(),
            ..field()
        }),
        Some(SCHEMA_ERROR)
    );
}
#[test]
fn path_flags_are_string_only_exclusive_and_anchor_game_relative() {
    assert_eq!(schema_error(path_field(CONFIG_FLAG_FILE_PATH)), None);
    assert_eq!(
        schema_error(path_field(
            CONFIG_FLAG_DIRECTORY_PATH | CONFIG_FLAG_GAME_RELATIVE | CONFIG_FLAG_ADVANCED
        )),
        None
    );
    for flags in [
        CONFIG_FLAG_FILE_PATH | CONFIG_FLAG_DIRECTORY_PATH,
        CONFIG_FLAG_GAME_RELATIVE,
        16,
    ] {
        assert_eq!(schema_error(path_field(flags)), Some(SCHEMA_ERROR));
    }
    assert_eq!(
        schema_error(ConfigField {
            flags: CONFIG_FLAG_FILE_PATH,
            ..field()
        }),
        Some(SCHEMA_ERROR)
    );
    assert_eq!(
        schema_error(ConfigField {
            flags: CONFIG_FLAG_ADVANCED,
            ..field()
        }),
        None
    );
}
#[test]
fn step_description_and_unit_are_bounded_presentation_metadata() {
    let number = ConfigField {
        default_value: ConfigValue {
            kind: VALUE_NUMBER,
            number: 1.0,
            ..integer(0)
        },
        number_min: 0.0,
        number_max: 4.0,
        ..field()
    };
    let description = "渲染宽度，影响画面清晰度。".as_bytes();
    assert_eq!(
        schema_error(ConfigField {
            step: 1.0,
            unit: Bytes::new(b"%"),
            description: Bytes::new(description),
            ..field()
        }),
        None
    );
    assert_eq!(
        schema_error(ConfigField {
            step: 0.25,
            ..number
        }),
        None
    );
    for step in [0.5, -1.0, f64::NAN] {
        assert_eq!(
            schema_error(ConfigField { step, ..field() }),
            Some(SCHEMA_ERROR)
        );
    }
    for step in [-0.25, f64::INFINITY] {
        assert_eq!(
            schema_error(ConfigField { step, ..number }),
            Some(SCHEMA_ERROR)
        );
    }
    assert_eq!(
        schema_error(ConfigField {
            step: 1.0,
            ..path_field(0)
        }),
        Some(SCHEMA_ERROR)
    );
    let long = [b'a'; 513];
    let long_unit = [b'u'; 17];
    for invalid in [
        ConfigField {
            description: Bytes::new(&long),
            ..field()
        },
        ConfigField {
            description: Bytes::new(b"line\nbreak"),
            ..field()
        },
        ConfigField {
            unit: Bytes::new(&long_unit),
            ..field()
        },
        ConfigField {
            unit: Bytes::new(b"\t"),
            ..field()
        },
    ] {
        assert_eq!(schema_error(invalid), Some(SCHEMA_ERROR));
    }
}
fn extent(width: u32, height: u32) -> Extent {
    Extent { width, height }
}
fn render_support() -> RenderExtentSupport {
    RenderExtentSupport {
        min: extent(320, 180),
        max: extent(3840, 3840),
        default: extent(1280, 720),
        flags: 0,
        reserved: 0,
    }
}
const RENDER_CAPS: u64 = CAP_CPU_SURFACE | CAP_RENDER_EXTENT;
#[test]
fn render_extent_support_requires_capability_and_ordered_bounds() {
    let support = render_support();
    support.validate(RENDER_CAPS).unwrap();
    RenderExtentSupport {
        default: Extent::default(),
        flags: RENDER_PRESERVE_ASPECT,
        ..support
    }
    .validate(RENDER_CAPS)
    .unwrap();
    RenderExtentSupport::default()
        .validate(CAP_CPU_SURFACE)
        .unwrap();
    let inverted = RenderExtentSupport {
        min: extent(320, 180),
        max: extent(160, 90),
        default: Extent::default(),
        ..support
    };
    assert_eq!(inverted.validate(RENDER_CAPS), Err(UNSUPPORTED));
    assert_eq!(support.validate(CAP_CPU_SURFACE), Err(UNSUPPORTED));
    for invalid in [
        RenderExtentSupport {
            min: extent(0, 180),
            ..support
        },
        RenderExtentSupport {
            max: extent(16385, 3840),
            ..support
        },
        RenderExtentSupport {
            default: extent(100, 720),
            ..support
        },
        RenderExtentSupport {
            default: extent(0, 720),
            ..support
        },
        RenderExtentSupport {
            flags: 2,
            ..support
        },
        RenderExtentSupport {
            reserved: 1,
            ..support
        },
    ] {
        assert_eq!(invalid.validate(RENDER_CAPS), Err(UNSUPPORTED));
    }
}
#[test]
fn requested_render_extent_must_fit_declared_range() {
    let support = render_support();
    support
        .validate_request(RENDER_CAPS, Extent::default())
        .unwrap();
    support
        .validate_request(RENDER_CAPS, extent(1920, 1080))
        .unwrap();
    RenderExtentSupport::default()
        .validate_request(CAP_CPU_SURFACE, Extent::default())
        .unwrap();
    assert_eq!(
        support.validate_request(RENDER_CAPS, extent(160, 1080)),
        Err(INVALID_ARGUMENT)
    );
    assert_eq!(
        support.validate_request(RENDER_CAPS, extent(1920, 4000)),
        Err(INVALID_ARGUMENT)
    );
    assert_eq!(
        RenderExtentSupport::default().validate_request(CAP_CPU_SURFACE, extent(1280, 720)),
        Err(INVALID_ARGUMENT)
    );
}
fn descriptor() -> Descriptor {
    Descriptor {
        header: Header::of::<Descriptor>(),
        family_id: Bytes::new(b"astra.emu.sena"),
        plugin_id: Bytes::new(b"sena-native-koikake"),
        version: Bytes::new(b"1.0.0"),
        capabilities: RENDER_CAPS,
        formats: std::ptr::null(),
        format_count: 0,
        configuration: std::ptr::null(),
        configuration_count: 0,
        display_name: Bytes::new(b"Sena"),
        family_name: Bytes::new(b"SoftPAL"),
        render: render_support(),
    }
}
#[test]
fn descriptor_validates_names_capabilities_and_render() {
    unsafe {
        descriptor().validate().unwrap();
        Descriptor {
            family_name: Bytes::new("Ethornell / BGI".as_bytes()),
            display_name: Bytes::new("模拟器".as_bytes()),
            ..descriptor()
        }
        .validate()
        .unwrap();
        let long = [b'n'; 65];
        for invalid in [Bytes::EMPTY, Bytes::new(&long), Bytes::new(b"tab\tname")] {
            assert_eq!(
                Descriptor {
                    display_name: invalid,
                    ..descriptor()
                }
                .validate(),
                Err(INVALID_ARGUMENT)
            );
            assert_eq!(
                Descriptor {
                    family_name: invalid,
                    ..descriptor()
                }
                .validate(),
                Err(INVALID_ARGUMENT)
            );
        }
        assert_eq!(
            Descriptor {
                capabilities: CAP_CPU_SURFACE,
                ..descriptor()
            }
            .validate(),
            Err(UNSUPPORTED)
        );
        assert_eq!(
            Descriptor {
                capabilities: CAP_RENDER_EXTENT,
                ..descriptor()
            }
            .validate(),
            Err(UNSUPPORTED)
        );
        let mut stale = descriptor();
        stale.header.version = 2;
        assert_eq!(stale.validate(), Err(UNSUPPORTED));
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
