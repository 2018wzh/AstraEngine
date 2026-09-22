use astra_platform::{AudioOutputLane, AudioOutputRequest, AudioWakeRegistration};
use astra_platform_common::NullAudioDevice;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

fn request() -> AudioOutputRequest {
    AudioOutputRequest {
        sample_rate: 48_000,
        channels: 2,
        chunk_frames: 480,
        max_buffered_frames: 480,
        start_paused: true,
        capture_samples: false,
    }
}

#[test]
fn null_device_consumes_pcm_on_clock_and_pause_preserves_queue() {
    let (device, mut producer) =
        NullAudioDevice::open(request(), AudioWakeRegistration::default()).unwrap();
    producer.submit(vec![0.5; 960]).unwrap();
    std::thread::sleep(Duration::from_millis(25));
    assert_eq!(producer.consumed_samples(), 0);
    device.resume().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while producer.consumed_samples() != 960 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(producer.consumed_samples(), 960);
    device.pause().unwrap();
    producer.submit(vec![0.25; 960]).unwrap();
    std::thread::sleep(Duration::from_millis(25));
    assert_eq!(producer.consumed_samples(), 960);
}

#[test]
fn cancellation_releases_blocked_pcm_before_device_close_and_reopen() {
    for _ in 0..8 {
        let wake = AudioWakeRegistration::default();
        let (device, mut producer) = NullAudioDevice::open(request(), wake.clone()).unwrap();
        producer.submit(vec![0.25; 960]).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (done, receive) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            producer.wait_for_capacity(960, &worker_stop).unwrap();
            done.send(()).unwrap();
        });
        assert!(receive.recv_timeout(Duration::from_millis(25)).is_err());
        stop.store(true, Ordering::Release);
        wake.notify();
        receive.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
        drop(device);
    }
}

#[test]
fn null_device_rejects_invalid_format_before_spawning() {
    let mut invalid = request();
    invalid.chunk_frames = 0;
    assert!(NullAudioDevice::open(invalid, AudioWakeRegistration::default()).is_err());
}
