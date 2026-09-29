use super::*;

fn submit(tracker: &mut PlaybackTracker, frames: usize, source: bool) {
    for _ in 0..frames {
        tracker.submitted_frame(source).unwrap();
    }
}

fn played(counters: &PlaybackCounters) -> u64 {
    counters
        .snapshot(AudioPlaybackClock::DeviceTimestamp)
        .played_source_frames
}

#[test]
fn queued_current_and_device_frames_remain_pending_until_timestamp_observation() {
    let counters = Arc::new(PlaybackCounters::default());
    let mut tracker = PlaybackTracker::new(counters.clone(), 48_000, 48_000);
    tracker
        .observe_device_delay(Duration::from_millis(10))
        .unwrap();
    counters.accept(960);
    assert_eq!(played(&counters), 0);
    submit(&mut tracker, 480, true);
    assert_eq!(played(&counters), 0);
    tracker
        .observe_device_delay(Duration::from_millis(5))
        .unwrap();
    assert_eq!(played(&counters), 240);
    tracker.observe_device_delay(Duration::ZERO).unwrap();
    assert_eq!(played(&counters), 480);
    assert_eq!(
        counters
            .snapshot(AudioPlaybackClock::DeviceTimestamp)
            .accepted_source_frames,
        960
    );
}

#[test]
fn underrun_silence_does_not_advance_family_source_frames() {
    let counters = Arc::new(PlaybackCounters::default());
    counters.accept(200);
    let mut tracker = PlaybackTracker::new(counters.clone(), 48_000, 48_000);
    submit(&mut tracker, 100, true);
    submit(&mut tracker, 200, false);
    submit(&mut tracker, 100, true);
    tracker.observe(250).unwrap();
    assert_eq!(played(&counters), 100);
    tracker.observe(350).unwrap();
    assert_eq!(played(&counters), 150);
    tracker.observe(400).unwrap();
    assert_eq!(played(&counters), 200);
}

#[test]
fn rational_mapping_keeps_delay_and_tail_without_per_callback_rounding_drift() {
    let counters = Arc::new(PlaybackCounters::default());
    counters.accept(48_000);
    counters.resampler_delay.store(64, Ordering::Release);
    let mut tracker = PlaybackTracker::new(counters.clone(), 48_000, 44_100);
    submit(&mut tracker, 44_100, true);
    tracker.observe(64).unwrap();
    assert_eq!(played(&counters), 0);
    for device in 65..=44_100 {
        tracker.observe(device).unwrap();
        assert_eq!(played(&counters), (device - 64) * 48_000 / 44_100);
    }
    assert!(played(&counters) < 48_000);
    submit(&mut tracker, 64, true);
    tracker.observe_test_consumption().unwrap();
    assert_eq!(played(&counters), 48_000);
}

#[test]
fn callback_before_acceptance_publication_cannot_overreport_or_move_backwards() {
    let counters = Arc::new(PlaybackCounters::default());
    let mut tracker = PlaybackTracker::new(counters.clone(), 48_000, 48_000);
    submit(&mut tracker, 480, true);
    tracker.observe(480).unwrap();
    assert_eq!(played(&counters), 0);
    counters.accept(480);
    tracker.observe(480).unwrap();
    assert_eq!(played(&counters), 480);
    tracker.observe(479).unwrap();
    assert_eq!(played(&counters), 480);
}

#[test]
fn device_clock_regression_is_rejected_without_retracting_played_frames() {
    let counters = Arc::new(PlaybackCounters::default());
    counters.accept(480);
    let mut tracker = PlaybackTracker::new(counters.clone(), 48_000, 48_000);
    submit(&mut tracker, 480, true);
    tracker.observe(240).unwrap();
    assert!(tracker.observe(200).is_err());
    assert_eq!(played(&counters), 240);
}

#[test]
fn count_overflow_and_unbounded_device_delay_are_rejected() {
    let counters = Arc::new(PlaybackCounters::default());
    counters.accept(u64::MAX);
    assert!(counters.check_accept(1).is_err());
    let mut tracker = PlaybackTracker::new(counters, 48_000, 192_000);
    assert!(tracker.observe_device_delay(Duration::MAX).is_err());
}
