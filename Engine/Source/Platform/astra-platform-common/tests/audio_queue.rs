use astra_platform::{AudioWakeRegistration, PlatformErrorCode};
use astra_platform_common::NativeAudioQueue;

#[test]
fn native_audio_queue_reports_overflow_and_underflow_without_mutexes() {
    let (mut producer, mut consumer, telemetry) =
        NativeAudioQueue::create(1, 2, AudioWakeRegistration::default()).expect("queue");
    let samples = vec![0.25, -0.25];
    let pointer = samples.as_ptr();
    let recycled = producer.try_submit_owned(samples).expect("fits");
    assert_ne!(recycled.as_ptr(), pointer);
    assert_eq!(
        producer.try_submit_owned(recycled).unwrap_err().code,
        PlatformErrorCode::QueueOverflow
    );

    assert_eq!(consumer.pop_sample(), Some(0.25));
    assert_eq!(consumer.pop_sample(), Some(-0.25));
    assert_eq!(consumer.pop_sample(), None);
    consumer.record_underflow();

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.underflow_count, 1);
    assert_eq!(snapshot.consumed_samples, 2);
    assert_eq!(snapshot.queued_samples, 0);
}

#[test]
fn native_audio_queue_moves_callback_chunks_in_bulk() {
    let (mut producer, mut consumer, telemetry) =
        NativeAudioQueue::create(2, 6, AudioWakeRegistration::default()).expect("queue");
    producer
        .try_submit_owned(vec![0.0, 0.1, 0.2, 0.3, 0.4, 0.5])
        .expect("bulk submit");
    let mut output = [-1.0; 8];

    assert_eq!(consumer.pop_samples(&mut output), 6);
    assert_eq!(&output[..6], &[0.0, 0.1, 0.2, 0.3, 0.4, 0.5]);
    assert_eq!(&output[6..], &[-1.0, -1.0]);
    assert_eq!(telemetry.snapshot().consumed_samples, 6);
}
