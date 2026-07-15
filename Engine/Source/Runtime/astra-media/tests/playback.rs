use astra_core::Hash256;
use astra_media::{
    AudioFramePacket, LateVideoPolicy, MediaPlaybackConfig, MediaPlaybackSession,
    MediaPlaybackState, MediaTrackKind, PlaybackTickRequest, VideoFramePacket,
};

fn video(generation: u64, sequence: u64, pts_us: u64) -> VideoFramePacket {
    VideoFramePacket {
        generation,
        sequence,
        resource_id: format!("video.frame.{sequence}"),
        pts_us,
        duration_us: 40_000,
        width: 320,
        height: 180,
        content_hash: Hash256::from_sha256(&sequence.to_le_bytes()),
    }
}

fn audio(generation: u64, sequence: u64, pts_us: u64) -> AudioFramePacket {
    AudioFramePacket {
        generation,
        sequence,
        resource_id: format!("audio.packet.{sequence}"),
        pts_us,
        duration_us: 20_000,
        sample_rate: 48_000,
        channels: 2,
        frame_count: 960,
        content_hash: Hash256::from_sha256(&sequence.to_le_bytes()),
    }
}

#[astra_headless_test::test]
fn playback_session_schedules_audio_master_video_pause_seek_and_eos() {
    let mut session = MediaPlaybackSession::new(MediaPlaybackConfig {
        duration_us: 120_000,
        ..MediaPlaybackConfig::default()
    })
    .unwrap();
    session.queue_audio(audio(1, 1, 0)).unwrap();
    session.queue_audio(audio(1, 2, 20_000)).unwrap();
    session.queue_video(video(1, 1, 0)).unwrap();
    session.queue_video(video(1, 2, 40_000)).unwrap();
    session.mark_eos(MediaTrackKind::Audio, 40_000).unwrap();
    session.mark_eos(MediaTrackKind::Video, 80_000).unwrap();
    session.play().unwrap();

    let first = session
        .tick(PlaybackTickRequest {
            sequence: 1,
            delta_us: 20_000,
            audio_playhead_us: Some(20_000),
        })
        .unwrap();
    assert_eq!(first.presented_video.unwrap().sequence, 1);
    assert_eq!(first.released_audio.len(), 1);
    session.pause().unwrap();
    assert!(session
        .tick(PlaybackTickRequest {
            sequence: 2,
            delta_us: 20_000,
            audio_playhead_us: Some(40_000),
        })
        .is_err());
    session.play().unwrap();
    let second = session
        .tick(PlaybackTickRequest {
            sequence: 2,
            delta_us: 20_000,
            audio_playhead_us: Some(40_000),
        })
        .unwrap();
    assert_eq!(second.presented_video.unwrap().sequence, 2);
    assert_eq!(second.released_audio.len(), 1);

    let generation = session.seek(80_000).unwrap();
    assert_eq!(generation, 2);
    assert_eq!(session.state, MediaPlaybackState::Seeking);
    assert!(session.queue_video(video(1, 1, 80_000)).is_err());
    session.mark_eos(MediaTrackKind::Audio, 80_000).unwrap();
    session.queue_video(video(2, 1, 80_000)).unwrap();
    session.mark_eos(MediaTrackKind::Video, 120_000).unwrap();
    session.complete_seek().unwrap();
    let ended = session
        .tick(PlaybackTickRequest {
            sequence: 3,
            delta_us: 40_000,
            audio_playhead_us: Some(120_000),
        })
        .unwrap();
    assert!(ended.ended);
    assert_eq!(session.state, MediaPlaybackState::Ended);
}

#[astra_headless_test::test]
fn playback_session_rejects_queue_clock_and_sync_errors_transactionally() {
    let mut session = MediaPlaybackSession::new(MediaPlaybackConfig {
        duration_us: 1_000_000,
        max_video_frames: 2,
        max_audio_packets: 2,
        max_video_lag_us: 10_000,
        late_video_policy: LateVideoPolicy::Block,
        ..MediaPlaybackConfig::default()
    })
    .unwrap();
    session.queue_audio(audio(1, 1, 0)).unwrap();
    session.queue_video(video(1, 1, 0)).unwrap();
    let before = session.deterministic_hash().unwrap();
    assert!(session.queue_audio(audio(1, 3, 20_000)).is_err());
    assert_eq!(session.deterministic_hash().unwrap(), before);
    session.play().unwrap();
    let before_tick = session.deterministic_hash().unwrap();
    assert!(session
        .tick(PlaybackTickRequest {
            sequence: 1,
            delta_us: 100_000,
            audio_playhead_us: Some(200_000),
        })
        .is_err());
    assert_eq!(session.deterministic_hash().unwrap(), before_tick);
}

#[astra_headless_test::test]
fn playback_drop_policy_is_explicit_and_cancel_releases_queues() {
    let mut session = MediaPlaybackSession::new(MediaPlaybackConfig {
        duration_us: 1_000_000,
        max_video_lag_us: 10_000,
        late_video_policy: LateVideoPolicy::Drop,
        ..MediaPlaybackConfig::default()
    })
    .unwrap();
    session.queue_audio(audio(1, 1, 0)).unwrap();
    session.queue_video(video(1, 1, 0)).unwrap();
    session.play().unwrap();
    let output = session
        .tick(PlaybackTickRequest {
            sequence: 1,
            delta_us: 100_000,
            audio_playhead_us: Some(100_000),
        })
        .unwrap();
    assert_eq!(output.dropped_video.len(), 1);
    assert!(output.presented_video.is_none());
    session.cancel().unwrap();
    assert_eq!(session.state, MediaPlaybackState::Cancelled);
    assert!(session.audio_queue.is_empty());
    assert!(session.video_queue.is_empty());
    assert!(session.cancel().is_err());
}

#[astra_headless_test::test]
fn playback_snapshot_restore_continues_identically_and_corruption_blocks() {
    let mut uninterrupted = MediaPlaybackSession::new(MediaPlaybackConfig {
        duration_us: 120_000,
        ..MediaPlaybackConfig::default()
    })
    .unwrap();
    uninterrupted.queue_audio(audio(1, 1, 0)).unwrap();
    uninterrupted.queue_audio(audio(1, 2, 20_000)).unwrap();
    uninterrupted.queue_video(video(1, 1, 0)).unwrap();
    uninterrupted.queue_video(video(1, 2, 40_000)).unwrap();
    uninterrupted
        .mark_eos(MediaTrackKind::Audio, 40_000)
        .unwrap();
    uninterrupted
        .mark_eos(MediaTrackKind::Video, 80_000)
        .unwrap();
    uninterrupted.play().unwrap();
    uninterrupted
        .tick(PlaybackTickRequest {
            sequence: 1,
            delta_us: 20_000,
            audio_playhead_us: Some(20_000),
        })
        .unwrap();

    let snapshot = uninterrupted.snapshot().unwrap();
    let mut restored = MediaPlaybackSession::restore(&snapshot).unwrap();
    let request = PlaybackTickRequest {
        sequence: 2,
        delta_us: 20_000,
        audio_playhead_us: Some(40_000),
    };
    assert_eq!(
        uninterrupted.tick(request.clone()).unwrap(),
        restored.tick(request).unwrap()
    );
    assert_eq!(
        uninterrupted.deterministic_hash().unwrap(),
        restored.deterministic_hash().unwrap()
    );

    let mut corrupt = snapshot;
    corrupt.truncate(corrupt.len() / 2);
    assert!(MediaPlaybackSession::restore(&corrupt).is_err());
}
