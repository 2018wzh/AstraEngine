use super::*;
use abi_stable::std_types::RVec;

#[test]
fn null_device_consumes_pcm_without_opening_a_native_device_and_closes() {
    let executor = HostAudioExecutor::new(AudioDeviceKind::Null, None);
    let sink = executor.sink();
    sink.configure(PcmFormatSpec {
        sample_rate: 48_000,
        channels: 2,
        format: PcmFormat::I16,
    })
    .into_result()
    .unwrap();
    assert!(matches!(
        *executor.state.stream.lock().unwrap(),
        Some(AudioOutputDevice::Null(_))
    ));
    let configured = executor.state.configured.lock().unwrap().unwrap();
    assert_eq!(configured.device_rate, 48_000);
    assert_eq!(configured.device_channels, 2);
    sink.write(PcmChunk::I16(RVec::from(vec![512; 960])))
        .into_result()
        .unwrap();
    let producer = executor
        .state
        .producer
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .clone();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !producer.is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "null device did not consume queued PCM"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let writer = std::thread::spawn(move || loop {
        let status = sink
            .write(PcmChunk::I16(RVec::from(vec![512; 960])))
            .into_result()
            .unwrap();
        if status != AudioWriteStatus::Accepted {
            return status;
        }
    });
    executor.close().unwrap();
    assert_eq!(writer.join().unwrap(), AudioWriteStatus::Cancelled);
    assert!(executor.state.stream.lock().unwrap().is_none());
    assert!(executor.sink().is_cancelled());
    executor.close().unwrap();
}

#[test]
fn unknown_audio_device_does_not_select_a_backend() {
    assert!(AudioDeviceKind::parse("silent-fallback").is_err());
}

#[test]
fn conversion_maps_mono_and_preserves_i16_extremes() {
    let values = convert_chunk(
        PcmChunk::I16(RVec::from(vec![i16::MIN, i16::MAX])),
        OutputFormat {
            source: PcmFormatSpec {
                sample_rate: 48_000,
                channels: 1,
                format: PcmFormat::I16,
            },
            device_rate: 48_000,
            device_channels: 2,
            device_sample_format: cpal::SampleFormat::F32,
        },
    )
    .unwrap();
    assert_eq!(values.len(), 4);
    assert_eq!(values[0], values[1]);
    assert!(values[0] <= -0.99);
    assert_eq!(values[2], values[3]);
    assert_eq!(values[2], 1.0);
}

#[test]
fn persistent_resampler_is_invariant_to_irregular_input_chunks() {
    let output = OutputFormat {
        source: PcmFormatSpec {
            sample_rate: 48_000,
            channels: 1,
            format: PcmFormat::F32,
        },
        device_rate: 24_000,
        device_channels: 1,
        device_sample_format: cpal::SampleFormat::F32,
    };
    let input = (0..10_000)
        .map(|index| ((index as f32) * 0.013).sin())
        .collect::<Vec<_>>();
    let mut whole = AudioConverter::new(output).unwrap();
    let expected = whole.push(input.clone()).unwrap();
    let mut irregular = AudioConverter::new(output).unwrap();
    let mut actual = Vec::new();
    for chunk in input.chunks(137) {
        actual.extend(irregular.push(chunk.to_vec()).unwrap());
    }
    assert_eq!(actual.len(), expected.len());
    for (left, right) in actual.iter().zip(expected.iter()) {
        assert!((left - right).abs() < 1.0e-6);
    }
}

#[test]
fn persistent_converter_maps_stereo_to_mono() {
    let output = OutputFormat {
        source: PcmFormatSpec {
            sample_rate: 48_000,
            channels: 2,
            format: PcmFormat::F32,
        },
        device_rate: 48_000,
        device_channels: 1,
        device_sample_format: cpal::SampleFormat::F32,
    };
    let mut converter = AudioConverter::new(output).unwrap();
    assert_eq!(
        converter.push(vec![1.0, -1.0, 0.4, 0.2]).unwrap(),
        vec![0.0, 0.3]
    );
}

#[test]
fn stream_error_is_reported_by_health_check() {
    let executor = HostAudioExecutor::new(AudioDeviceKind::Native, None);
    executor.state.stream_failed.store(true, Ordering::Release);
    assert_eq!(
        executor.check_health().unwrap_err(),
        "ASTRA_EMU_AUDIO_DEVICE_STREAM_FAILED"
    );
}

#[test]
fn device_consumer_advances_blocks_inside_one_callback_buffer() {
    let (sender, receiver) = bounded(AUDIO_QUEUE_CHUNKS);
    sender.send(OutputPcm::Converted(vec![0.25, 0.5])).unwrap();
    sender.send(OutputPcm::Converted(vec![0.75])).unwrap();
    let (retired, _reclaimer) = rtrb::RingBuffer::new(AUDIO_QUEUE_CHUNKS + 2);
    let mut consumer = DeviceConsumer {
        receiver,
        current: None,
        retired,
        cursor: 0,
        channels: 1,
        tracker: None,
        last_callback: None,
        _completion: ConsumerCompletion(None),
    };
    let mut output = [0.0; 4];
    consumer.fill_f32(&mut output).unwrap();
    assert_eq!(output, [0.25, 0.5, 0.75, 0.0]);
}

#[test]
fn close_cancels_unconfigured_sink_without_device_access() {
    let executor = HostAudioExecutor::new(AudioDeviceKind::Native, None);
    assert!(!executor.is_configured());
    executor.close().unwrap();
    assert!(executor.sink().is_cancelled());
}

fn manually_configured_audio() -> (HostAudioExecutor, DeviceConsumer) {
    let executor = HostAudioExecutor::new(AudioDeviceKind::Null, None);
    let output = OutputFormat {
        source: PcmFormatSpec {
            sample_rate: 48_000,
            channels: 2,
            format: PcmFormat::F32,
        },
        device_rate: 48_000,
        device_channels: 2,
        device_sample_format: cpal::SampleFormat::F32,
    };
    let (producer, receiver) = bounded(AUDIO_QUEUE_CHUNKS);
    *executor.state.configured.lock().unwrap() = Some(output);
    *executor.state.converter.lock().unwrap() = Some(AudioConverter::new(output).unwrap());
    *executor.state.producer.lock().unwrap() = Some(producer);
    let (retired, reclaimer) = rtrb::RingBuffer::new(AUDIO_QUEUE_CHUNKS + 2);
    *executor.state.retired.lock() = Some(reclaimer);
    let mut consumer = DeviceConsumer {
        receiver,
        current: None,
        retired,
        cursor: 0,
        channels: 2,
        tracker: None,
        last_callback: None,
        _completion: ConsumerCompletion(None),
    };
    consumer.configure_tracking(output, &executor.state);
    (executor, consumer)
}

#[test]
fn progress_tracks_partial_current_block_and_rejects_old_sink_after_close() {
    let (executor, mut consumer) = manually_configured_audio();
    let sink = executor.sink();
    let before = sink.playback_progress().into_result().unwrap();
    assert_eq!(before.accepted_source_frames, 0);
    assert_eq!(before.played_source_frames, 0);
    sink.write(PcmChunk::F32(vec![0.25; 960].into()))
        .into_result()
        .unwrap();
    let mut output = [0.0; 240];
    consumer.fill_f32(&mut output).unwrap();
    assert_eq!(
        sink.playback_progress()
            .into_result()
            .unwrap()
            .played_source_frames,
        0
    );
    consumer.observe_null().unwrap();
    let partial = sink.playback_progress().into_result().unwrap();
    assert_eq!(partial.accepted_source_frames, 480);
    assert_eq!(partial.played_source_frames, 120);
    assert_eq!(partial.clock, AudioPlaybackClock::TestConsumption);
    assert!(sink
        .configure(PcmFormatSpec {
            sample_rate: 44_100,
            channels: 1,
            format: PcmFormat::I16
        })
        .into_result()
        .is_err());
    executor.close().unwrap();
    assert_eq!(
        sink.playback_progress()
            .into_result()
            .unwrap_err()
            .code
            .as_str(),
        "ASTRA_EMU_AUDIO_CLOSED"
    );
    let (next, _) = manually_configured_audio();
    assert_eq!(
        next.sink()
            .playback_progress()
            .into_result()
            .unwrap()
            .accepted_source_frames,
        0
    );
    assert!(sink.playback_progress().into_result().is_err());
}

#[test]
fn cancellation_wakes_queued_and_serialized_writers_without_accepting_pending_pcm() {
    let (executor, _consumer) = manually_configured_audio();
    let sink = executor.sink();
    for _ in 0..AUDIO_QUEUE_CHUNKS {
        assert_eq!(
            sink.write(PcmChunk::F32(vec![0.25; 960].into()))
                .into_result()
                .unwrap(),
            AudioWriteStatus::Accepted
        );
    }
    let start = Arc::new(std::sync::Barrier::new(4));
    let writers = (0..3).map(|_| {
        let writer_sink = executor.sink();
        let start = start.clone();
        std::thread::spawn(move || {
            start.wait();
            writer_sink.write(PcmChunk::F32(vec![0.5; 960].into())).into_result().unwrap()
        })
    }).collect::<Vec<_>>();
    start.wait();
    // One writer owns conversion/queue admission; others wait on serialization.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while executor.state.converter.try_lock().is_ok() {
        assert!(std::time::Instant::now() < deadline, "writer never entered admission");
        std::thread::yield_now();
    }
    sink.cancel().into_result().unwrap();
    for writer in writers {
        assert_eq!(writer.join().unwrap(), AudioWriteStatus::Cancelled);
    }
    let internal = executor
        .state
        .progress
        .snapshot(AudioPlaybackClock::TestConsumption);
    assert_eq!(
        internal.accepted_source_frames,
        AUDIO_QUEUE_CHUNKS as u64 * 480
    );
    assert_eq!(internal.played_source_frames, 0);
    assert_eq!(
        sink.playback_progress()
            .into_result()
            .unwrap_err()
            .code
            .as_str(),
        "ASTRA_EMU_AUDIO_CANCELLED"
    );
}

#[test]
fn disconnected_write_is_not_accepted_and_device_failure_is_not_progress() {
    let (executor, consumer) = manually_configured_audio();
    drop(consumer);
    let sink = executor.sink();
    assert!(sink
        .write(PcmChunk::F32(vec![0.25; 960].into()))
        .into_result()
        .is_err());
    assert_eq!(
        executor
            .state
            .progress
            .snapshot(AudioPlaybackClock::TestConsumption)
            .accepted_source_frames,
        0
    );
    assert!(sink.is_cancelled());
    assert_eq!(
        sink.playback_progress().into_result().unwrap_err().code(),
        "ASTRA_EMU_AUDIO_CANCELLED"
    );
    executor.state.stream_failed.store(true, Ordering::Release);
    assert_eq!(
        sink.playback_progress()
            .into_result()
            .unwrap_err()
            .code
            .as_str(),
        "ASTRA_EMU_AUDIO_DEVICE_STREAM_FAILED"
    );
}

#[test]
fn real_converter_retains_nonintegral_delay_and_tail_in_source_frame_progress() {
    let (executor, mut consumer) = manually_configured_audio();
    let output = OutputFormat {
        source: PcmFormatSpec {
            sample_rate: 48_000,
            channels: 2,
            format: PcmFormat::F32,
        },
        device_rate: 44_100,
        device_channels: 1,
        device_sample_format: cpal::SampleFormat::F32,
    };
    let converter = AudioConverter::new(output).unwrap();
    let delay = converter.output_delay() as u64;
    executor
        .state
        .progress
        .resampler_delay
        .store(delay, Ordering::Release);
    *executor.state.configured.lock().unwrap() = Some(output);
    *executor.state.converter.lock().unwrap() = Some(converter);
    consumer.configure_tracking(output, &executor.state);
    let sink = executor.sink();
    sink.write(PcmChunk::F32(vec![0.25; 960].into()))
        .into_result()
        .unwrap();
    assert!(consumer.receiver.is_empty());
    let retained = sink.playback_progress().into_result().unwrap();
    assert_eq!(retained.accepted_source_frames, 480);
    assert_eq!(retained.played_source_frames, 0);
    sink.write(PcmChunk::F32(vec![0.25; 3138].into()))
        .into_result()
        .unwrap();
    let mut samples = vec![0.0; 4096];
    consumer.fill_f32(&mut samples).unwrap();
    consumer.observe_null().unwrap();
    let result = sink.playback_progress().into_result().unwrap();
    assert_eq!(result.accepted_source_frames, 2049);
    assert!(result.played_source_frames > 0);
    assert!(result.played_source_frames < 2048);
    let held = result.played_source_frames;
    consumer.fill_f32(&mut samples).unwrap();
    consumer.observe_null().unwrap();
    assert_eq!(
        sink.playback_progress()
            .into_result()
            .unwrap()
            .played_source_frames,
        held
    );
}

#[cfg(not(target_os = "windows"))]
#[test]
fn unsupported_native_progress_keeps_basic_callback_playback() {
    let (mut executor, mut consumer) = manually_configured_audio();
    Arc::get_mut(&mut executor.state).unwrap().device_kind = AudioDeviceKind::Native;
    consumer.tracker = None;
    let sink = executor.sink();
    sink.write(PcmChunk::F32(vec![0.25; 4].into()))
        .into_result()
        .unwrap();
    let mut output = [0.0; 4];
    consumer.fill_f32(&mut output).unwrap();
    assert_eq!(output, [0.25; 4]);
    assert_eq!(
        sink.playback_progress()
            .into_result()
            .unwrap_err()
            .code
            .as_str(),
        "ASTRA_EMU_AUDIO_PROGRESS_UNSUPPORTED"
    );
    assert!(!sink.is_cancelled());
}

#[test]
fn close_preserves_progress_failure_before_the_next_health_poll() {
    let executor = HostAudioExecutor::new(AudioDeviceKind::Null, None);
    let sink = executor.sink();
    sink.configure(PcmFormatSpec {
        sample_rate: 48_000,
        channels: 2,
        format: PcmFormat::I16,
    })
    .into_result()
    .unwrap();
    executor
        .state
        .progress_failed
        .store(true, Ordering::Release);
    executor.state.cancelled.store(true, Ordering::Release);
    assert_eq!(
        executor.close().unwrap_err(),
        "ASTRA_EMU_AUDIO_PROGRESS_INVALID"
    );
    assert!(executor.state.stream.lock().unwrap().is_none());
    assert!(executor.state.producer.lock().unwrap().is_none());
    assert!(sink.is_cancelled());
    assert_eq!(
        sink.playback_progress().into_result().unwrap_err().code(),
        "ASTRA_EMU_AUDIO_PROGRESS_INVALID"
    );
}
