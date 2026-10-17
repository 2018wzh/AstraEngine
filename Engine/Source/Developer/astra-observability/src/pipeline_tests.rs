use super::*;

#[test]
fn full_and_disconnected_queue_preserve_drop_count_and_critical_warning() {
    let root = tempfile::tempdir().unwrap();
    let critical_path = root.path().join("critical.jsonl");
    let (writer, receiver) = mpsc::sync_channel(1);
    let dropped = Arc::new(AtomicU64::new(0));
    let layer = StableJsonLayer {
        session_id: "queue-test".into(),
        role: "test".into(),
        ring: Arc::new(Mutex::new(RingBuffer::new(8, 8192))),
        writer,
        critical: Some(Arc::new(Mutex::new(rotating_file(
            critical_path.clone(),
            16384,
            2,
        )))),
        dropped: Arc::clone(&dropped),
        console_json: false,
    };
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(event = "test.queued");
        tracing::info!(event = "test.full");
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
        let critical = fs::read_to_string(&critical_path).unwrap();
        assert!(critical.contains("observability.queue.saturated"));
        assert!(critical.contains("\"dropped_count\":1"));
        assert!(matches!(
            receiver.try_recv().unwrap(),
            WriterCommand::Record(_)
        ));
        drop(receiver);
        tracing::info!(event = "test.disconnected");
        assert_eq!(dropped.load(Ordering::Relaxed), 2);
        let critical = fs::read_to_string(&critical_path).unwrap();
        assert!(critical.contains("\"dropped_count\":2"));
    });
}

#[test]
fn native_family_events_preserve_values_but_spans_and_other_targets_stay_sanitized() {
    let (writer, receiver) = mpsc::sync_channel(8);
    let layer = StableJsonLayer {
        session_id: "family-test".into(),
        role: "test".into(),
        ring: Arc::new(Mutex::new(RingBuffer::new(8, 8192))),
        writer,
        critical: None,
        dropped: Arc::new(AtomicU64::new(0)),
        console_json: false,
    };
    let subscriber = tracing_subscriber::registry().with(layer);
    let debug_value = ("https://example.invalid/gpu", r"C:\game\shader.bin");
    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!(target: "astra_emu::family", "native_span", location = r"C:\game\asset.bin");
        let _entered = span.enter();
        tracing::info!(target: "astra_emu::family",
            event = "family.diagnostics", core_target = "https://example.invalid/native",
            fields = ?debug_value, dropped_fields = 7_u64,
            flag = true, signed = -2_i64, unsigned = 3_u64, fraction = 1.25_f64,
            message = "ordinary message");
        tracing::info!(target: "astra_emu::family::extra", event = "prefix", location = "https://example.invalid/private");
        tracing::info!(target: "other_product", event = "ordinary", location = r"C:\game\private.bin");
    });
    let next = || {
        let WriterCommand::Record(encoded) = receiver.try_recv().unwrap() else {
            panic!("expected serialized event");
        };
        serde_json::from_str::<LogEventV1>(&encoded).unwrap()
    };
    let native = next();
    assert_eq!(
        native.fields["core_target"],
        "https://example.invalid/native"
    );
    assert_eq!(native.fields["fields"], format!("{debug_value:?}"));
    assert_eq!(native.fields["dropped_fields"], 7);
    assert_eq!(native.fields["flag"], true);
    assert_eq!(native.fields["signed"], -2);
    assert_eq!(native.fields["unsigned"], 3);
    assert_eq!(native.fields["fraction"], 1.25);
    assert_eq!(native.fields["message"], "ordinary message");
    assert_eq!(native.span_stack[0].fields["location"], "[redacted]");
    assert_eq!(next().fields["location"], "[redacted]");
    assert_eq!(next().fields["location"], "[redacted]");
}
