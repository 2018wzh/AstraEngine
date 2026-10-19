use super::*;
struct NoSecrets;
impl SecretResolver for NoSecrets {
    fn resolve(
        &self,
        _: &str,
    ) -> Result<String, astra_emu_translation::TranslationError> {
        Err(astra_emu_translation::TranslationError::SecretUnavailable)
    }
}
fn bridge(source: &str, epoch: u64) -> TextReplacementBridge {
    TextReplacementBridge::new(
        epoch,
        None,
        Arc::new(NoSecrets),
        vec![LocalExtension {
            id: format!("test-{epoch}"),
            enabled: true,
            configuration: serde_json::json!({}),
            source: source.into(),
        }],
    )
    .unwrap()
}
fn invoke(
    bridge: &TextReplacementBridge,
    epoch: u64,
    context: u64,
    id: u64,
    bytes: &mut [u8],
    ranges: &mut [abi::TextRange],
) -> (abi::Status, usize) {
    let item = abi::TextItem {
        id: 7,
        kind: abi::TEXT_DIALOGUE,
        reserved: 0,
        text: abi::Bytes::new(b"source"),
    };
    let request = abi::TextRequest {
        session_epoch: epoch,
        context_epoch: context,
        request_id: id,
        items: &item,
        item_count: 1,
    };
    let mut output = abi::TextResult {
        bytes: bytes.as_mut_ptr(),
        byte_capacity: bytes.len(),
        byte_count: 0,
        items: ranges.as_mut_ptr(),
        item_capacity: ranges.len(),
        item_count: 0,
    };
    let service = bridge.sink();
    let status = unsafe {
        service.request.unwrap()(
            service.context,
            &request,
            &mut output,
            &mut abi::Error::default(),
        )
    };
    (status, output.byte_count)
}
#[test]
fn capacity_retry_retains_one_result_without_rerunning_luau() {
    let bridge = bridge(
        "capacity_hits = (capacity_hits or 0) + 1; return function(item) return item.text .. tostring(capacity_hits) end",
        1001,
    );
    let mut none = [];
    let mut ranges = [abi::TextRange {
        id: 0,
        offset: 0,
        len: 0,
    }];
    assert_eq!(
        invoke(&bridge, 1001, 1, 1, &mut none, &mut ranges),
        (abi::CAPACITY, 7)
    );
    let mut bytes = [0; 32];
    assert_eq!(
        invoke(&bridge, 1001, 1, 1, &mut bytes, &mut ranges),
        (abi::OK, 7)
    );
    assert_eq!(&bytes[..7], b"source1");
    let service = bridge.sink();
    assert_eq!(
        unsafe { service.acknowledge.unwrap()(service.context, 1001, 1, 1) },
        abi::OK
    );
    assert_eq!(
        invoke(&bridge, 1001, 1, 2, &mut bytes, &mut ranges),
        (abi::OK, 7)
    );
    assert_eq!(&bytes[..7], b"source2");
    bridge.close().unwrap();
}
#[test]
fn failed_luau_operation_retries_same_request_and_context_invalidation_rejects_old_result() {
    let bridge = bridge(
        "retry_hits=(retry_hits or 0)+1; return function(item) if retry_hits == 1 then error('failure') end return item.text end",
        1002,
    );
    let mut bytes = [0; 32];
    let mut ranges = [abi::TextRange {
        id: 0,
        offset: 0,
        len: 0,
    }];
    assert_eq!(
        invoke(&bridge, 1002, 1, 1, &mut bytes, &mut ranges).0,
        abi::CORE_ERROR
    );
    assert_eq!(
        invoke(&bridge, 1002, 1, 1, &mut bytes, &mut ranges).0,
        abi::OK
    );
    let service = bridge.sink();
    unsafe {
        service.invalidate.unwrap()(service.context, 1002, 2);
    }
    assert_eq!(
        invoke(&bridge, 1002, 1, 1, &mut bytes, &mut ranges).0,
        abi::CANCELLED
    );
    assert_eq!(
        invoke(&bridge, 1002, 2, 2, &mut bytes, &mut ranges).0,
        abi::OK
    );
    bridge.close().unwrap();
}
#[test]
fn cancellation_before_registration_is_not_lost() {
    let bridge = bridge("return function(item) while true do end end", 1003);
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let worker_barrier = barrier.clone();
    let worker_bridge = bridge.clone();
    let worker = std::thread::spawn(move || {
        worker_barrier.wait();
        let mut bytes = [0; 32];
        let mut ranges = [abi::TextRange {
            id: 0,
            offset: 0,
            len: 0,
        }];
        invoke(&worker_bridge, 1003, 1, 1, &mut bytes, &mut ranges).0
    });
    bridge.cancel();
    barrier.wait();
    assert_eq!(worker.join().unwrap(), abi::CANCELLED);
    bridge.close().unwrap();
}
#[test]
fn native_cancel_interrupts_running_luau_without_queuing_behind_it() {
    let bridge = bridge("return function(item) while true do end end", 1004);
    let worker_bridge = bridge.clone();
    let worker = std::thread::spawn(move || {
        let mut bytes = [0; 32];
        let mut ranges = [abi::TextRange {
            id: 0,
            offset: 0,
            len: 0,
        }];
        invoke(&worker_bridge, 1004, 1, 1, &mut bytes, &mut ranges).0
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while bridge.state.active.load(Ordering::Acquire) == 0 {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    bridge.cancel();
    assert_eq!(worker.join().unwrap(), abi::CANCELLED);
    bridge.close().unwrap();
}

#[test]
fn native_cancel_stops_tls_wait_and_releases_the_shared_luau_queue() {
    use std::io::Read;
    use std::net::{Shutdown, TcpListener};
    use std::sync::mpsc;

    struct FixtureSecret;
    impl SecretResolver for FixtureSecret {
        fn resolve(
            &self,
            _: &str,
        ) -> Result<String, astra_emu_translation::TranslationError> {
            Ok("public-fixture-token".into())
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let (hello, observed_hello) = mpsc::sync_channel(1);
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let (mut socket, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "TLS client did not connect"
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("TLS accept failed: {error}"),
            }
        };
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut record = [0u8; 4096];
        socket.read_exact(&mut record[..1]).unwrap();
        assert_eq!(record[0], 0x16, "client must begin a real TLS handshake");
        hello.send(()).unwrap();
        // Deliberately withhold ServerHello. No certificate trust bypass or
        // provider mock is involved; cancelling must stop this network wait.
        let closed = loop {
            match socket.read(&mut record) {
                Ok(0) => break true,
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                    ) =>
                {
                    break true;
                }
                Err(_) => break false,
            }
        };
        let _ = socket.shutdown(Shutdown::Both);
        closed
    });
    let profile = TranslationProfile {
        endpoint: format!("https://{address}"),
        model: "fixture-model".into(),
        timeout_ms: 30_000,
        ..TranslationProfile::default()
    };
    let first =
        TextReplacementBridge::new(2001, Some(profile), Arc::new(FixtureSecret), Vec::new())
            .unwrap();
    let queued = bridge(
        "return function(item) return item.text .. '-after' end",
        2002,
    );
    let first_call = first.clone();
    let request = std::thread::spawn(move || {
        let mut bytes = [0; 32];
        let mut ranges = [abi::TextRange {
            id: 0,
            offset: 0,
            len: 0,
        }];
        invoke(&first_call, 2001, 1, 1, &mut bytes, &mut ranges).0
    });
    observed_hello.recv_timeout(Duration::from_secs(5)).unwrap();
    let second_call = queued.clone();
    let (completed, receive) = mpsc::sync_channel(1);
    let second = std::thread::spawn(move || {
        let mut bytes = [0; 32];
        let mut ranges = [abi::TextRange {
            id: 0,
            offset: 0,
            len: 0,
        }];
        let (status, size) = invoke(&second_call, 2002, 1, 1, &mut bytes, &mut ranges);
        completed.send((status, bytes[..size].to_vec())).unwrap();
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while queued.state.active.load(Ordering::Acquire) == 0 {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(matches!(receive.try_recv(), Err(mpsc::TryRecvError::Empty)));
    first.cancel();
    assert_eq!(request.join().unwrap(), abi::CANCELLED);
    assert_eq!(
        receive.recv_timeout(Duration::from_secs(5)).unwrap(),
        (abi::OK, b"source-after".to_vec())
    );
    second.join().unwrap();
    first.close().unwrap();
    queued.close().unwrap();
    assert!(
        server.join().unwrap(),
        "TLS transport remained active after native cancellation"
    );
}
