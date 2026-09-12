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
