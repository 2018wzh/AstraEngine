use super::*;

#[test]
fn disconnected_queue_is_only_cancellation_when_requested() {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    drop(rx);
    let (_, finished_rx) = std::sync::mpsc::sync_channel(1);
    let audio = AudioBridge {
        tx,
        finished_rx,
        error: Arc::new(Mutex::new(None)),
        cancelled: Arc::new(AtomicBool::new(false)),
        handle: None,
    };
    assert_eq!(
        audio
            .send(MixerCommand::StopAll { fade_ms: 0 })
            .unwrap_err()
            .code,
        "ASTRA_EMU_ARTEMIS_AUDIO_QUEUE"
    );
    audio.cancelled.store(true, Ordering::Release);
    assert!(audio
        .send(MixerCommand::StopAll { fade_ms: 0 })
        .unwrap()
        .is_break());
    audio.close().unwrap();
}
