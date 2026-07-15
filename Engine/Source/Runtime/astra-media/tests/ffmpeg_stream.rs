#![cfg(feature = "ffmpeg-vcpkg")]

use astra_media::{
    DecodedMediaPacket, FfmpegAudioOutputFormat, FfmpegDecodedPacket, FfmpegPlaybackDecoder,
    FfmpegStreamLimits, MediaPipelineLimits, MediaPlaybackPipeline, MediaPlaybackSession,
    PlaybackTickRequest, QueuedMediaOutput,
};

#[astra_headless_test::test]
fn ffmpeg_audio_stream_produces_timestamped_packets_accepted_by_scheduler() {
    let mut decoder = FfmpegPlaybackDecoder::open(
        "mp3",
        &fixture_bytes("t-rex-roar.mp3"),
        FfmpegStreamLimits::default(),
    )
    .unwrap();
    let mut session = MediaPlaybackSession::new(decoder.playback_config()).unwrap();
    let mut previous_end = 0;
    let mut packet_count = 0;

    while let Some(decoded) = decoder.read_next().unwrap() {
        let FfmpegDecodedPacket::Audio { packet, pcm_s16le } = decoded else {
            panic!("audio fixture produced a video packet");
        };
        assert_eq!(packet.sequence, packet_count + 1);
        assert!(packet.pts_us >= previous_end);
        assert_eq!(
            packet.content_hash,
            astra_core::Hash256::from_sha256(&pcm_s16le)
        );
        assert_eq!(
            pcm_s16le.len(),
            packet.frame_count as usize * packet.channels as usize * 2
        );
        previous_end = packet.pts_us + packet.duration_us;
        session.queue_audio(packet).unwrap();
        packet_count += 1;
        if session.audio_queue.len() == session.config.max_audio_packets {
            break;
        }
    }
    assert!(packet_count > 4);
}

#[astra_headless_test::test]
fn ffmpeg_video_stream_is_monotonic_seekable_and_cancellable() {
    let bytes = fixture_bytes("flower.mp4");
    let mut decoder =
        FfmpegPlaybackDecoder::open("mp4", &bytes, FfmpegStreamLimits::default()).unwrap();
    let config = decoder.playback_config();
    assert!(config.has_video);
    let mut previous_pts = 0;
    let mut video_count = 0;
    while video_count < 8 {
        let decoded = decoder
            .read_next()
            .unwrap()
            .expect("video fixture ended early");
        if let FfmpegDecodedPacket::Video { packet, bgra8 } = decoded {
            assert_eq!(packet.sequence, video_count + 1);
            assert!(packet.pts_us >= previous_pts);
            assert_eq!(
                bgra8.len(),
                packet.width as usize * packet.height as usize * 4
            );
            assert_eq!(
                packet.content_hash,
                astra_core::Hash256::from_sha256(&bgra8)
            );
            previous_pts = packet.pts_us;
            video_count += 1;
        }
    }

    let seek_target = config.duration_us / 2;
    assert_eq!(decoder.seek(seek_target).unwrap(), 2);
    let post_seek = loop {
        let packet = decoder
            .read_next()
            .unwrap()
            .expect("seek did not produce another frame");
        if let FfmpegDecodedPacket::Video { packet, .. } = packet {
            break packet;
        }
    };
    assert_eq!(post_seek.generation, 2);
    assert_eq!(post_seek.sequence, 1);
    assert!(post_seek.pts_us >= seek_target);

    decoder.cancel().unwrap();
    assert!(decoder.read_next().is_err());
    assert!(decoder.cancel().is_err());
}

#[astra_headless_test::test]
fn ffmpeg_stream_rejects_corrupt_input_and_invalid_budgets() {
    assert!(
        FfmpegPlaybackDecoder::open("mp4", b"not a container", FfmpegStreamLimits::default())
            .is_err()
    );
    let limits = FfmpegStreamLimits {
        max_pending_packets: 0,
        ..FfmpegStreamLimits::default()
    };
    assert!(FfmpegPlaybackDecoder::open("mp3", &fixture_bytes("t-rex-roar.mp3"), limits).is_err());
}

#[astra_headless_test::test]
fn ffmpeg_stream_drains_to_eos_with_single_packet_backpressure() {
    let limits = FfmpegStreamLimits {
        max_pending_packets: 1,
        ..FfmpegStreamLimits::default()
    };
    let mut decoder =
        FfmpegPlaybackDecoder::open("mp3", &fixture_bytes("t-rex-roar.mp3"), limits).unwrap();
    let duration_us = decoder.playback_config().duration_us;
    let mut count = 0_u64;
    let mut final_end_us = 0;
    while let Some(decoded) = decoder.read_next().unwrap() {
        let FfmpegDecodedPacket::Audio { packet, .. } = decoded else {
            panic!("audio fixture produced a video packet");
        };
        final_end_us = packet.pts_us + packet.duration_us;
        count += 1;
    }
    assert!(count > 32);
    assert!(
        final_end_us <= duration_us,
        "final packet end {final_end_us} exceeded declared duration {duration_us}"
    );
    assert!(decoder.read_next().unwrap().is_none());
}

#[astra_headless_test::test]
fn ffmpeg_packets_flow_through_scheduler_with_owned_payloads() {
    let mut audio_decoder = FfmpegPlaybackDecoder::open(
        "mp3",
        &fixture_bytes("t-rex-roar.mp3"),
        FfmpegStreamLimits::default(),
    )
    .unwrap();
    let mut audio_pipeline = MediaPlaybackPipeline::new(
        audio_decoder.playback_config(),
        MediaPipelineLimits::default(),
    )
    .unwrap();
    let mut first_audio_end = None;
    for _ in 0..8 {
        let decoded = audio_decoder.read_next().unwrap().unwrap();
        let queued = audio_pipeline.queue_decoded(decoded).unwrap();
        let QueuedMediaOutput::Audio { packet, pcm_s16le } = queued else {
            panic!("audio stream buffered a video frame");
        };
        assert_eq!(
            packet.content_hash,
            astra_core::Hash256::from_sha256(&pcm_s16le)
        );
        first_audio_end.get_or_insert(packet.pts_us + packet.duration_us);
    }
    audio_pipeline.play().unwrap();
    let output = audio_pipeline
        .tick(PlaybackTickRequest {
            sequence: 1,
            delta_us: 1,
            audio_playhead_us: first_audio_end,
        })
        .unwrap();
    assert_eq!(output.scheduler.released_audio.len(), 1);

    let mut video_decoder = FfmpegPlaybackDecoder::open(
        "mp4",
        &fixture_bytes("flower.mp4"),
        FfmpegStreamLimits::default(),
    )
    .unwrap();
    let mut video_pipeline = MediaPlaybackPipeline::new(
        video_decoder.playback_config(),
        MediaPipelineLimits::default(),
    )
    .unwrap();
    let mut first_video_pts = None;
    let mut first_mp4_audio_end = None;
    while first_video_pts.is_none() || first_mp4_audio_end.is_none() {
        let decoded = video_decoder.read_next().unwrap().unwrap();
        match &decoded {
            DecodedMediaPacket::Video { packet, .. } => {
                first_video_pts.get_or_insert(packet.pts_us);
            }
            DecodedMediaPacket::Audio { packet, .. } => {
                first_mp4_audio_end.get_or_insert(packet.pts_us + packet.duration_us);
            }
        }
        video_pipeline.queue_decoded(decoded).unwrap();
    }
    video_pipeline.play().unwrap();
    let output = video_pipeline
        .tick(PlaybackTickRequest {
            sequence: 1,
            delta_us: 1,
            audio_playhead_us: first_mp4_audio_end,
        })
        .unwrap();
    let presented = output.presented_video.unwrap();
    assert!(presented.packet.pts_us >= first_video_pts.unwrap());
    assert_eq!(
        presented.packet.content_hash,
        astra_core::Hash256::from_sha256(&presented.bgra8)
    );
}

#[astra_headless_test::test]
fn media_pipeline_rejects_payload_tamper_without_partial_queue() {
    let mut decoder = FfmpegPlaybackDecoder::open(
        "mp3",
        &fixture_bytes("t-rex-roar.mp3"),
        FfmpegStreamLimits::default(),
    )
    .unwrap();
    let mut pipeline =
        MediaPlaybackPipeline::new(decoder.playback_config(), MediaPipelineLimits::default())
            .unwrap();
    let mut decoded = decoder.read_next().unwrap().unwrap();
    let DecodedMediaPacket::Audio { pcm_s16le, .. } = &mut decoded else {
        panic!("audio fixture produced video");
    };
    pcm_s16le[0] ^= 0xff;
    assert!(pipeline.queue_decoded(decoded).is_err());
    assert!(pipeline.scheduler().audio_queue.is_empty());
}

#[astra_headless_test::test]
fn ffmpeg_stream_resamples_to_explicit_native_audio_format() {
    let mut decoder = FfmpegPlaybackDecoder::open_with_audio_output(
        "mp3",
        &fixture_bytes("t-rex-roar.mp3"),
        FfmpegStreamLimits::default(),
        Some(FfmpegAudioOutputFormat {
            sample_rate: 48_000,
            channels: 1,
        }),
    )
    .unwrap();
    let packet = decoder.read_next().unwrap().unwrap();
    let DecodedMediaPacket::Audio { packet, pcm_s16le } = packet else {
        panic!("audio fixture produced video");
    };
    assert_eq!(packet.sample_rate, 48_000);
    assert_eq!(packet.channels, 1);
    assert_eq!(pcm_s16le.len(), packet.frame_count as usize * 2);
    assert!(decoder
        .configure_audio_output(FfmpegAudioOutputFormat {
            sample_rate: 44_100,
            channels: 2,
        })
        .is_err());
}

#[astra_headless_test::test]
fn media_pipeline_payload_budget_blocks_without_partial_state() {
    let mut decoder = FfmpegPlaybackDecoder::open(
        "mp3",
        &fixture_bytes("t-rex-roar.mp3"),
        FfmpegStreamLimits::default(),
    )
    .unwrap();
    let mut pipeline = MediaPlaybackPipeline::new(
        decoder.playback_config(),
        MediaPipelineLimits {
            max_live_audio_bytes: 1,
            max_live_video_bytes: 1,
        },
    )
    .unwrap();
    assert!(pipeline
        .queue_decoded(decoder.read_next().unwrap().unwrap())
        .is_err());
    assert!(pipeline.scheduler().audio_queue.is_empty());
    assert!(MediaPlaybackPipeline::new(
        decoder.playback_config(),
        MediaPipelineLimits {
            max_live_audio_bytes: 0,
            max_live_video_bytes: 1,
        },
    )
    .is_err());
}

fn fixture_bytes(file: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../Fixtures/PublicDomainMedia")
        .join(file);
    std::fs::read(path).unwrap()
}
