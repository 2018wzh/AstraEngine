use super::*;
use crate::native::text::{ServiceContext, TextBridge};
use astra_emu_abi as abi;
use std::{ffi::c_void, sync::atomic::AtomicU64};

#[derive(Default)]
struct Service {
    mode: AtomicUsize,
    calls: AtomicUsize,
    acknowledgements: AtomicUsize,
    invalidations: AtomicUsize,
    epoch: AtomicU64,
    ids: parking_lot::Mutex<Vec<u64>>,
    kinds: parking_lot::Mutex<Vec<Vec<u32>>>,
    inputs: parking_lot::Mutex<Vec<Vec<String>>>,
}
impl Service {
    fn bridge(&self) -> TextBridge {
        TextBridge(Arc::new(ServiceContext {
            session: 1,
            epoch: AtomicU64::new(1),
            cancelled: AtomicBool::new(false),
            waiting: AtomicBool::new(false),
            text: Some(abi::TextService {
                context: (self as *const Self).cast_mut().cast(),
                request: Some(transform),
                acknowledge: Some(acknowledge),
                invalidate: Some(invalidate),
            }),
            window: None,
            host_cancel: None,
        }))
    }
}
unsafe extern "C" fn transform(
    context: *mut c_void,
    request: *const abi::TextRequest,
    output: *mut abi::TextResult,
    error: *mut abi::Error,
) -> abi::Status {
    let service = unsafe { &*context.cast::<Service>() };
    let request = unsafe { &*request };
    let result = unsafe { &mut *output };
    let items = unsafe { std::slice::from_raw_parts(request.items, request.item_count) };
    service.calls.fetch_add(1, Ordering::AcqRel);
    service.ids.lock().push(request.request_id);
    service
        .kinds
        .lock()
        .push(items.iter().map(|item| item.kind).collect());
    service.inputs.lock().push(
        items
            .iter()
            .map(|item| {
                let bytes = unsafe { std::slice::from_raw_parts(item.text.data, item.text.len) };
                std::str::from_utf8(bytes).unwrap().to_owned()
            })
            .collect(),
    );
    match service.mode.load(Ordering::Acquire) {
        0 => {
            unsafe { (*error).set("TEST_TRANSLATION_FAILED", "host service error") };
            return abi::CORE_ERROR;
        }
        2 => {
            while service.epoch.load(Ordering::Acquire) <= request.context_epoch {
                std::thread::sleep(Duration::from_millis(1));
            }
            // Deliberately return a late success. The core must reject it even if
            // a broken host fails to return CANCELLED after invalidation.
        }
        _ => {}
    }
    let replacement = match service.mode.load(Ordering::Acquire) {
        3 => "\u{10ffff}".to_owned(),
        4 => "T".repeat(2048),
        _ => "Translated".to_owned(),
    };
    result.byte_count = replacement.len() * items.len();
    result.item_count = items.len();
    if result.byte_capacity < result.byte_count || result.item_capacity < result.item_count {
        return abi::CAPACITY;
    }
    for (index, item) in items.iter().enumerate() {
        unsafe {
            std::ptr::copy_nonoverlapping(
                replacement.as_ptr(),
                result.bytes.add(index * replacement.len()),
                replacement.len(),
            );
            result.items.add(index).write(abi::TextRange {
                id: item.id,
                offset: index * replacement.len(),
                len: replacement.len(),
            });
        }
    }
    abi::OK
}
unsafe extern "C" fn acknowledge(context: *mut c_void, _: u64, _: u64, _: u64) -> abi::Status {
    unsafe { &*context.cast::<Service>() }
        .acknowledgements
        .fetch_add(1, Ordering::AcqRel);
    abi::OK
}
unsafe extern "C" fn invalidate(context: *mut c_void, _: u64, epoch: u64) {
    let service = unsafe { &*context.cast::<Service>() };
    service.epoch.store(epoch, Ordering::Release);
    service.invalidations.fetch_add(1, Ordering::AcqRel);
}

#[test]
fn semantic_hook_failure_retries_same_event_without_reexecuting_vm() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".message 1  Speaker Hello\r\n.select First:done Second:done\r\n.label done\r\n.end\r\n",
    );
    let service = Box::<Service>::default();
    let mut provider = fixture::provider();
    let (_, mut session) = provider
        .open_native_session(
            request(root.path(), Sink::default()),
            astra_emu_abi::Extent::default(),
            Some(service.bridge()),
        )
        .unwrap();
    let failure = session.advance(16_666_667, &[]).unwrap_err();
    assert_eq!(failure.code(), "TEST_TRANSLATION_FAILED");
    assert!(session.hook_failed && session.pending.is_some());
    assert!(!session.poisoned);
    assert!(session.message.is_none());
    let pc = session.vm.state().pc_line;
    let instructions = session.vm.state().instruction_count;
    let request_id = session.pending.as_ref().unwrap().id;
    service.mode.store(1, Ordering::Release);
    session.native_retry().unwrap();
    assert_eq!(session.vm.state().pc_line, pc);
    assert_eq!(session.vm.state().instruction_count, instructions);
    assert_eq!(
        session.message,
        Some(("Translated".into(), Some("Translated".into())))
    );
    assert_eq!(*service.ids.lock(), vec![request_id, request_id]);
    assert_eq!(
        service.kinds.lock()[0],
        vec![abi::TEXT_DIALOGUE, abi::TEXT_SPEAKER]
    );
    session.advance(0, &[key(KeyCode::F5)]).unwrap();
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(
        service.kinds.lock().last().unwrap(),
        &vec![abi::TEXT_CHOICE, abi::TEXT_CHOICE]
    );
    assert_eq!(
        session.choice_display().unwrap().unwrap().0,
        vec!["Translated", "Translated"]
    );
    let epoch = session
        .replacement
        .as_ref()
        .unwrap()
        .0
        .epoch
        .load(Ordering::Acquire);
    session.advance(0, &[key(KeyCode::F9)]).unwrap();
    assert!(
        session
            .replacement
            .as_ref()
            .unwrap()
            .0
            .epoch
            .load(Ordering::Acquire)
            > epoch
    );
    assert!(session.choice_text.is_none());
    assert_eq!(session.message.as_ref().unwrap().0, "Translated");
    Box::new(session).close().unwrap();
}

#[test]
fn independent_invalidation_rejects_a_late_success() {
    for ending in [false, true] {
        let service = Arc::new(Service::default());
        service.mode.store(2, Ordering::Release);
        let bridge = service.bridge();
        let context = bridge.0.clone();
        let held = service.clone();
        let worker = std::thread::spawn(move || {
            let _held = held;
            bridge.transform(7, &[(abi::TEXT_DIALOGUE, "Original")])
        });
        until(|| service.calls.load(Ordering::Acquire) == 1);
        if ending {
            context.cancel();
        } else {
            context.invalidate();
        }
        assert_eq!(
            worker.join().unwrap().unwrap_err().code(),
            "ASTRA_EMU_MUSICA_TEXT_CANCELLED"
        );
        assert_eq!(service.acknowledgements.load(Ordering::Acquire), 0);
        assert_eq!(context.cancelled.load(Ordering::Acquire), ending);
    }
}

#[test]
fn hook_glyph_error_is_a_renderer_failure_not_a_retryable_service_error() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".message 1   Hello\r\n.end\r\n");
    let service = Box::<Service>::default();
    service.mode.store(3, Ordering::Release);
    let (_, mut session) = fixture::provider()
        .open_native_session(
            request(root.path(), Sink::default()),
            astra_emu_abi::Extent::default(),
            Some(service.bridge()),
        )
        .unwrap();
    assert_eq!(
        session.advance(16_666_667, &[]).unwrap_err().code(),
        "ASTRA_TEXT_GLYPH_MISSING"
    );
    assert!(session.poisoned);
    assert!(session.pending.is_none());
    Box::new(session).close().unwrap();
}

#[test]
fn capacity_retry_keeps_request_identity_and_acknowledges_once() {
    let service = Box::<Service>::default();
    service.mode.store(4, Ordering::Release);
    let bridge = service.bridge();
    let result = bridge
        .transform(17, &[(abi::TEXT_DIALOGUE, "Source")])
        .unwrap();
    assert_eq!(result, vec!["T".repeat(2048)]);
    assert_eq!(*service.ids.lock(), vec![17, 17]);
    assert_eq!(service.acknowledgements.load(Ordering::Acquire), 1);
}

#[test]
fn host_startup_cancellation_is_checked_without_a_session_handle() {
    unsafe extern "C" fn cancelled(context: *mut c_void, _: u64) -> u32 {
        u32::from(unsafe { &*context.cast::<AtomicBool>() }.load(Ordering::Acquire))
    }
    let service = Box::<Service>::default();
    let stop = AtomicBool::new(true);
    let mut bridge = service.bridge();
    Arc::get_mut(&mut bridge.0).unwrap().host_cancel = Some(abi::CancellationService {
        context: (&stop as *const AtomicBool).cast_mut().cast(),
        is_cancelled: Some(cancelled),
    });
    assert_eq!(
        bridge.check_cancelled().unwrap_err().code(),
        "ASTRA_EMU_MUSICA_CANCELLED"
    );
    assert_eq!(service.calls.load(Ordering::Acquire), 0);
    assert_eq!(service.invalidations.load(Ordering::Acquire), 1);
}

fn native_table(root: &std::path::Path) {
    let record = |source: &[u8], text: &str| {
        serde_json::json!({
            "source_hash": crate::musica_display_hash(source),
            "source_sha256": astra_core::Hash256::from_sha256(source).to_hex(),
            "text": text,
        })
    };
    std::fs::write(
        root.join("translation.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 2, "source_encoding": "gbk", "hash_collision_policy": "first_record",
            "names": [record(b"Speaker", "Native speaker"), record(b"First", "Native choice")],
            "dialogue": [record(b"Hello", "Native dialogue")],
        }))
        .unwrap(),
    )
    .unwrap();
}

fn localized_request(root: &std::path::Path, mode: &str) -> OpenRequest {
    let mut request = request(root, Sink::default());
    request.configuration.push(ConfigEntry {
        id: "translation_table".into(),
        value: ConfigValue::String("translation.json".into()),
    });
    request.configuration.push(ConfigEntry {
        id: "translation_mode".into(),
        value: ConfigValue::Enum(mode.into()),
    });
    request
}

#[test]
fn native_display_precedes_hook_and_retry_keeps_original_vm_and_hook_identity() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".message 1  Speaker Hello\r\n.select First:done Second:done\r\n.label done\r\n.end\r\n",
    );
    native_table(root.path());
    let service = Box::<Service>::default();
    let (_, mut session) = fixture::provider()
        .open_native_session(
            localized_request(root.path(), "replace"),
            astra_emu_abi::Extent::default(),
            Some(service.bridge()),
        )
        .unwrap();
    assert_eq!(
        session.advance(16_666_667, &[]).unwrap_err().code(),
        "TEST_TRANSLATION_FAILED"
    );
    let original = &session.vm.state().backlog[0];
    assert_eq!(original.text, "Hello");
    assert_eq!(original.speaker.as_deref(), Some("Speaker"));
    assert_eq!(
        service.inputs.lock()[0],
        vec!["Native dialogue", "Native speaker"]
    );
    let instructions = session.vm.state().instruction_count;
    service.mode.store(1, Ordering::Release);
    session.native_retry().unwrap();
    assert_eq!(session.vm.state().instruction_count, instructions);
    {
        let ids = service.ids.lock();
        assert_eq!(ids[0], ids[1]);
    }
    assert_eq!(
        service.inputs.lock()[1],
        vec!["Native dialogue", "Native speaker"]
    );
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(
        service.inputs.lock().last().unwrap(),
        &vec!["Native choice", "Second"]
    );
    assert_eq!(
        session.vm.choice_display().unwrap().unwrap().0,
        vec!["First", "Second"]
    );
    Box::new(session).close().unwrap();
}

#[test]
fn overlay_tab_release_and_save_restore_preserve_chinese_base_and_cached_text() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".message 1  Speaker Hello\r\n.message 2   Unmatched\r\n.end\r\n",
    );
    native_table(root.path());
    let (_, mut session) = fixture::provider()
        .open_native_session(
            localized_request(root.path(), "overlay"),
            astra_emu_abi::Extent::default(),
            None,
        )
        .unwrap();
    session.advance(16_666_667, &[]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "Hello");
    assert_eq!(
        session.message.as_ref().unwrap().1.as_deref(),
        Some("Native speaker")
    );
    assert_eq!(
        session.translation_overlay.as_deref(),
        Some("Native dialogue")
    );
    session.advance(0, &[key(KeyCode::Tab)]).unwrap();
    assert!(session.translation_visible);
    let mut released = key(KeyCode::Tab);
    if let FamilyEvent::Key { state, .. } = &mut released {
        *state = KeyState::Released;
    }
    session.advance(0, &[released.clone()]).unwrap();
    assert!(!session.translation_visible);
    session.advance(0, &[key(KeyCode::F5)]).unwrap();
    session.advance(0, &[released]).unwrap();
    assert!(session.translation_visible);
    session.advance(0, &[key(KeyCode::F9)]).unwrap();
    assert!(!session.translation_visible);
    assert_eq!(
        session.translation_overlay.as_deref(),
        Some("Native dialogue")
    );
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "Unmatched");
    assert_eq!(
        session.translation_overlay.as_deref(),
        Some("Native dialogue")
    );
    Box::new(session).close().unwrap();
}
