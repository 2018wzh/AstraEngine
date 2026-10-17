use super::*;
struct NoSecrets;
impl SecretResolver for NoSecrets { fn resolve(&self, _: &str) -> Result<String, astra_emu_translation_openai_compatible::TranslationError> { Err(astra_emu_translation_openai_compatible::TranslationError::SecretUnavailable) } }
fn bridge(source: &str, epoch: u64) -> TextReplacementBridge {
    TextReplacementBridge::new(epoch, None, Arc::new(NoSecrets), vec![LocalExtension { id: format!("test-{epoch}"), enabled: true, configuration: serde_json::json!({}), source: source.into() }]).unwrap()
}
fn invoke(bridge: &TextReplacementBridge, epoch: u64, context: u64, id: u64, bytes: &mut [u8], ranges: &mut [abi::TextRange]) -> (abi::Status, usize) {
    let item = abi::TextItem { id: 7, kind: abi::TEXT_DIALOGUE, reserved: 0, text: abi::Bytes::new(b"source") };
    let request = abi::TextRequest { session_epoch: epoch, context_epoch: context, request_id: id, items: &item, item_count: 1 };
    let mut output = abi::TextResult { bytes: bytes.as_mut_ptr(), byte_capacity: bytes.len(), byte_count: 0, items: ranges.as_mut_ptr(), item_capacity: ranges.len(), item_count: 0 };
    let service = bridge.sink();
    let status = unsafe { service.request.unwrap()(service.context, &request, &mut output, &mut abi::Error::default()) };
    (status, output.byte_count)
}
#[test]
fn capacity_retry_retains_one_result_without_rerunning_luau() {
    let bridge = bridge("capacity_hits = (capacity_hits or 0) + 1; return function(item) return item.text .. tostring(capacity_hits) end", 1001);
    let mut none = [];
    let mut ranges = [abi::TextRange { id: 0, offset: 0, len: 0 }];
    assert_eq!(invoke(&bridge, 1001, 1, 1, &mut none, &mut ranges), (abi::CAPACITY, 7));
    let mut bytes = [0; 32];
    assert_eq!(invoke(&bridge, 1001, 1, 1, &mut bytes, &mut ranges), (abi::OK, 7));
    assert_eq!(&bytes[..7], b"source1");
    let service = bridge.sink();
    assert_eq!(unsafe { service.acknowledge.unwrap()(service.context, 1001, 1, 1) }, abi::OK);
    assert_eq!(invoke(&bridge, 1001, 1, 2, &mut bytes, &mut ranges), (abi::OK, 7));
    assert_eq!(&bytes[..7], b"source2");
    bridge.close().unwrap();
}
#[test]
fn failed_luau_operation_retries_same_request_and_context_invalidation_rejects_old_result() {
    let bridge = bridge("retry_hits=(retry_hits or 0)+1; return function(item) if retry_hits == 1 then error('failure') end return item.text end", 1002);
    let mut bytes = [0; 32]; let mut ranges = [abi::TextRange { id: 0, offset: 0, len: 0 }];
    assert_eq!(invoke(&bridge, 1002, 1, 1, &mut bytes, &mut ranges).0, abi::CORE_ERROR);
    assert_eq!(invoke(&bridge, 1002, 1, 1, &mut bytes, &mut ranges).0, abi::OK);
    let service = bridge.sink(); unsafe { service.invalidate.unwrap()(service.context, 1002, 2); }
    assert_eq!(invoke(&bridge, 1002, 1, 1, &mut bytes, &mut ranges).0, abi::CANCELLED);
    assert_eq!(invoke(&bridge, 1002, 2, 2, &mut bytes, &mut ranges).0, abi::OK);
    bridge.close().unwrap();
}
#[test]
fn cancellation_before_registration_is_not_lost() {
    let bridge = bridge("return function(item) while true do end end", 1003);
    let barrier = Arc::new(std::sync::Barrier::new(2)); let worker_barrier = barrier.clone(); let worker_bridge = bridge.clone();
    let worker = std::thread::spawn(move || {
        worker_barrier.wait();
        let mut bytes = [0; 32]; let mut ranges = [abi::TextRange { id: 0, offset: 0, len: 0 }];
        invoke(&worker_bridge, 1003, 1, 1, &mut bytes, &mut ranges).0
    });
    bridge.cancel(); barrier.wait();
    assert_eq!(worker.join().unwrap(), abi::CANCELLED);
    bridge.close().unwrap();
}
#[test]
fn native_cancel_interrupts_running_luau_without_queuing_behind_it() {
    let bridge = bridge("return function(item) while true do end end", 1004); let worker_bridge = bridge.clone();
    let worker = std::thread::spawn(move || {
        let mut bytes = [0; 32]; let mut ranges = [abi::TextRange { id: 0, offset: 0, len: 0 }];
        invoke(&worker_bridge, 1004, 1, 1, &mut bytes, &mut ranges).0
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while bridge.state.active.load(Ordering::Acquire) == 0 { assert!(std::time::Instant::now() < deadline); std::thread::yield_now(); }
    bridge.cancel(); assert_eq!(worker.join().unwrap(), abi::CANCELLED); bridge.close().unwrap();
}
