use super::*;
use astra_emu_abi as abi;
use crate::native::text::{ServiceContext, TextBridge};
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
}
impl Service {
    fn bridge(&self) -> TextBridge {
        TextBridge(Arc::new(ServiceContext { session: 1, epoch: AtomicU64::new(1), cancelled: AtomicBool::new(false), waiting: AtomicBool::new(false),
            text: Some(abi::TextService { context: (self as *const Self).cast_mut().cast(), request: Some(transform), acknowledge: Some(acknowledge), invalidate: Some(invalidate) }),
            window: None, host_cancel: None }))
    }
}
unsafe extern "C" fn transform(context: *mut c_void, request: *const abi::TextRequest, output: *mut abi::TextResult, error: *mut abi::Error) -> abi::Status {
    let service = unsafe { &*context.cast::<Service>() };
    let request = unsafe { &*request };
    let result = unsafe { &mut *output };
    let items = unsafe { std::slice::from_raw_parts(request.items, request.item_count) };
    service.calls.fetch_add(1, Ordering::AcqRel);
    service.ids.lock().push(request.request_id);
    service.kinds.lock().push(items.iter().map(|item| item.kind).collect());
    match service.mode.load(Ordering::Acquire) {
        0 => { unsafe { (*error).set("TEST_TRANSLATION_FAILED", "host service error") }; return abi::CORE_ERROR; }
        2 => {
            while service.epoch.load(Ordering::Acquire) <= request.context_epoch { std::thread::sleep(Duration::from_millis(1)); }
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
    result.byte_count = replacement.len() * items.len(); result.item_count = items.len();
    if result.byte_capacity < result.byte_count || result.item_capacity < result.item_count { return abi::CAPACITY; }
    for (index, item) in items.iter().enumerate() {
        unsafe {
            std::ptr::copy_nonoverlapping(replacement.as_ptr(), result.bytes.add(index * replacement.len()), replacement.len());
            result.items.add(index).write(abi::TextRange { id: item.id, offset: index * replacement.len(), len: replacement.len() });
        }
    }
    abi::OK
}
unsafe extern "C" fn acknowledge(context: *mut c_void, _: u64, _: u64, _: u64) -> abi::Status {
    unsafe { &*context.cast::<Service>() }.acknowledgements.fetch_add(1, Ordering::AcqRel); abi::OK
}
unsafe extern "C" fn invalidate(context: *mut c_void, _: u64, epoch: u64) {
    let service = unsafe { &*context.cast::<Service>() };
    service.epoch.store(epoch, Ordering::Release); service.invalidations.fetch_add(1, Ordering::AcqRel);
}

#[test]
fn semantic_hook_failure_retries_same_event_without_reexecuting_vm() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".message 1  Speaker Hello\r\n.select First:done Second:done\r\n.label done\r\n.end\r\n");
    let service = Box::<Service>::default();
    let mut provider = fixture::provider();
    let (_, mut session) = provider.open_native_session(request(root.path(), Sink::default()), Some(service.bridge())).unwrap();
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
    assert_eq!(session.message, Some(("Translated".into(), Some("Translated".into()))));
    assert_eq!(*service.ids.lock(), vec![request_id, request_id]);
    assert_eq!(service.kinds.lock()[0], vec![abi::TEXT_DIALOGUE, abi::TEXT_SPEAKER]);
    session.advance(0, &[key(KeyCode::F5)]).unwrap();
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(service.kinds.lock().last().unwrap(), &vec![abi::TEXT_CHOICE, abi::TEXT_CHOICE]);
    assert_eq!(session.choice_display().unwrap().unwrap().0, vec!["Translated", "Translated"]);
    let epoch = session.replacement.as_ref().unwrap().0.epoch.load(Ordering::Acquire);
    session.advance(0, &[key(KeyCode::F9)]).unwrap();
    assert!(session.replacement.as_ref().unwrap().0.epoch.load(Ordering::Acquire) > epoch);
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
        if ending { context.cancel(); } else { context.invalidate(); }
        assert_eq!(worker.join().unwrap().unwrap_err().code(), "ASTRA_EMU_MUSICA_TEXT_CANCELLED");
        assert_eq!(service.acknowledgements.load(Ordering::Acquire), 0);
        assert_eq!(context.cancelled.load(Ordering::Acquire), ending);
    }
}

#[test]
fn hook_glyph_error_is_a_renderer_failure_not_a_retryable_service_error() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".message 1   Hello\r\n.end\r\n");
    let service = Box::<Service>::default();
    service.mode.store(3, Ordering::Release);
    let (_, mut session) = fixture::provider().open_native_session(request(root.path(), Sink::default()), Some(service.bridge())).unwrap();
    assert_eq!(session.advance(16_666_667, &[]).unwrap_err().code(), "ASTRA_TEXT_GLYPH_MISSING");
    assert!(session.poisoned);
    assert!(session.pending.is_none());
    Box::new(session).close().unwrap();
}

#[test]
fn capacity_retry_keeps_request_identity_and_acknowledges_once() {
    let service = Box::<Service>::default();
    service.mode.store(4, Ordering::Release);
    let bridge = service.bridge();
    let result = bridge.transform(17, &[(abi::TEXT_DIALOGUE, "Source")]).unwrap();
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
        context: (&stop as *const AtomicBool).cast_mut().cast(), is_cancelled: Some(cancelled),
    });
    assert_eq!(bridge.check_cancelled().unwrap_err().code(), "ASTRA_EMU_MUSICA_CANCELLED");
    assert_eq!(service.calls.load(Ordering::Acquire), 0);
    assert_eq!(service.invalidations.load(Ordering::Acquire), 1);
}
