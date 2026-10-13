use super::*;

fn test_video(scope: astra_runtime::TaskScope) -> ActiveVideoStream {
    ActiveVideoStream {
        request: NativeVnVideoRequest {
            scope,
            layer: "movie".into(),
            asset_id: "asset:/test.webm".into(),
            codec: "webm".into(),
            encoded_bytes: vec![1].into(),
            encoded_length: 1,
            alpha_millionths: 1_000_000,
            looping: false,
            fence: None,
        },
        session: astra_player_core::PlayerHostResourceId(41),
        decoded_end_us: 0,
        presented_end_us: 0,
        loop_duration_us: None,
        expected_frame_count: None,
        expected_decoded_byte_count: None,
        decoded_byte_count: 0,
        pending_frame: None,
        next_frame: 0,
        next_request_sequence: 1,
        reached_end: false,
        loop_index: 0,
        started_at_ms: 0,
    }
}

#[test]
fn restored_video_keeps_elapsed_time_and_survives_an_immediate_resave() {
    let mut media = NativeVnProductMediaHost::default();
    media.clock.advance(200).unwrap();
    media.clock.advance(240).unwrap();
    let mut video = test_video(astra_runtime::TaskScope::new());
    video.started_at_ms = 10;
    media.active_videos.push(video);
    let saved = media.snapshot();
    media.restore(saved.clone()).unwrap();
    assert!(media.active_videos.is_empty());
    let resaved = media.snapshot();
    assert_eq!(
        serde_json::to_value(&resaved).unwrap(),
        serde_json::to_value(&saved).unwrap()
    );
    media.restore(resaved).unwrap();
    let now = media.clock.advance(0).unwrap();
    assert_eq!(now - media.restored_videos[0].started_at_ms, 30);
    let now = media.clock.advance(15).unwrap();
    assert_eq!(now - media.restored_videos[0].started_at_ms, 45);
    let before = serde_json::to_value(media.snapshot()).unwrap();
    for variant in 0..3 {
        let mut invalid = media.snapshot();
        match variant {
            0 => invalid.schema = "astra.player.native_vn_media_snapshot.v3".into(),
            1 => invalid.active_videos[0].started_at_ms = invalid.playback_time_ms + 1,
            _ => invalid.timeline.last_time_ms = Some(invalid.playback_time_ms + 1),
        }
        assert!(media.restore(invalid).is_err());
        assert_eq!(serde_json::to_value(media.snapshot()).unwrap(), before);
    }
}

#[test]
fn restore_preserves_existing_and_active_decode_stream_closes() {
    let mut host = NativeVnProductMediaHost::new(8);
    let snapshot = host.snapshot();
    host.pending_video_closes
        .push(astra_player_core::PlayerHostResourceId(40));
    host.active_videos
        .push(test_video(astra_runtime::TaskScope::new()));
    let mut invalid = snapshot.clone();
    invalid.schema = "invalid".into();
    assert!(host.restore(invalid).is_err());
    assert_eq!(host.active_videos.len(), 1);
    assert_eq!(host.pending_video_closes.len(), 1);
    host.restore(snapshot.clone()).unwrap();
    assert!(host.active_videos.is_empty());
    assert_eq!(
        host.pending_video_closes,
        vec![
            astra_player_core::PlayerHostResourceId(40),
            astra_player_core::PlayerHostResourceId(41)
        ]
    );
    host.restore(snapshot).unwrap();
    assert_eq!(host.pending_video_closes.len(), 2);
}

fn pcm(id: &str, samples: usize) -> PcmAsset {
    PcmAsset::from_canonical_samples(id, vec![0.0; samples]).unwrap()
}

#[test]
fn decoded_audio_recency_evicts_the_oldest_live_generation() {
    let mut host = NativeVnProductMediaHost::new(8);
    host.max_decoded_cache_bytes = 32;
    let first = AudioCacheKey::new("first", "wav", 4);
    let second = AudioCacheKey::new("second", "wav", 4);
    let third = AudioCacheKey::new("third", "wav", 4);

    host.cache_audio(first.clone(), pcm("first", 4));
    host.cache_audio(second.clone(), pcm("second", 4));
    assert!(host.cached_audio(&first).is_some());
    host.cache_audio(third.clone(), pcm("third", 4));

    assert!(host.decoded_audio_cache.contains_key(&first));
    assert!(!host.decoded_audio_cache.contains_key(&second));
    assert!(host.decoded_audio_cache.contains_key(&third));
    assert_eq!(host.decoded_audio_cache_bytes, 32);
}

#[test]
fn decoded_audio_recency_queue_compacts_stale_generations() {
    let mut host = NativeVnProductMediaHost::new(8);
    let key = AudioCacheKey::new("stable", "wav", 4);
    host.cache_audio(key.clone(), pcm("stable", 4));
    for _ in 0..128 {
        assert!(host.touch_cached_audio(&key));
    }
    host.compact_audio_recency_if_needed();

    assert!(host.decoded_audio_lru.len() <= 33);
    let live_epoch = host.decoded_audio_cache[&key].last_access_epoch;
    assert_eq!(host.decoded_audio_lru.back(), Some(&(key, live_epoch)));
}
