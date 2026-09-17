#![cfg(feature = "ffmpeg-vcpkg")]

use astra_media::{
    DecodedMediaPacket, FfmpegAudioOutputFormat, FfmpegDecodedPacket, FfmpegPlaybackDecoder,
    FfmpegStreamLimits, MediaPipelineLimits, MediaPlaybackPipeline, MediaPlaybackSession,
    PlaybackTickRequest, QueuedMediaOutput,
};

#[test]
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
        let FfmpegDecodedPacket::Audio { packet, samples } = decoded else {
            panic!("audio fixture produced a video packet");
        };
        assert_eq!(packet.sequence, packet_count + 1);
        assert!(packet.pts_us >= previous_end);
        assert_eq!(
            samples.len(),
            packet.frame_count as usize * packet.channels as usize
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

#[test]
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

#[test]
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

#[test]
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

#[test]
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
        let QueuedMediaOutput::Audio { packet, samples } = queued else {
            panic!("audio stream buffered a video frame");
        };
        assert_eq!(
            samples.len(),
            packet.frame_count as usize * packet.channels as usize
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
        presented.bgra8.len(),
        presented.packet.width as usize * presented.packet.height as usize * 4
    );
}

#[test]
fn media_pipeline_rejects_malformed_payload_without_partial_queue() {
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
    let DecodedMediaPacket::Audio { samples, .. } = &mut decoded else {
        panic!("audio fixture produced video");
    };
    samples.pop();
    assert!(pipeline.queue_decoded(decoded).is_err());
    assert!(pipeline.scheduler().audio_queue.is_empty());
}

#[test]
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
    let DecodedMediaPacket::Audio { packet, samples } = packet else {
        panic!("audio fixture produced video");
    };
    assert_eq!(packet.sample_rate, 48_000);
    assert_eq!(packet.channels, 1);
    assert_eq!(samples.len(), packet.frame_count as usize);
    assert!(decoder
        .configure_audio_output(FfmpegAudioOutputFormat {
            sample_rate: 44_100,
            channels: 2,
        })
        .is_err());
}

#[test]
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

#[test]
fn resampled_stream_preserves_full_duration_without_packet_gaps() {
    // A complete 200 ms PCM source gives an exact output frame count at both rates.
    let mut source = b"RIFF".to_vec();
    source.extend(3236_u32.to_le_bytes());
    source.extend(b"WAVEfmt ");
    source.extend(16_u32.to_le_bytes());
    source.extend(1_u16.to_le_bytes());
    source.extend(1_u16.to_le_bytes());
    source.extend(8000_u32.to_le_bytes());
    source.extend(16000_u32.to_le_bytes());
    source.extend(2_u16.to_le_bytes());
    source.extend(16_u16.to_le_bytes());
    source.extend(b"data");
    source.extend(3200_u32.to_le_bytes());
    for _ in 0..1600 {
        source.extend(8192_i16.to_le_bytes());
    }
    for rate in [4000, 48000] {
        let mut decoder = FfmpegPlaybackDecoder::open_with_audio_output(
            "wav",
            &source,
            FfmpegStreamLimits::default(),
            Some(FfmpegAudioOutputFormat {
                sample_rate: rate,
                channels: 1,
            }),
        )
        .unwrap();
        let mut frames = 0_u64;
        let mut packets = 0;
        while let Some(decoded) = decoder.read_next().unwrap() {
            let DecodedMediaPacket::Audio { packet, samples } = decoded else {
                panic!("unexpected video")
            };
            let expected_pts = frames * 1_000_000 / u64::from(rate);
            assert!(packet.pts_us.abs_diff(expected_pts) <= 1_000_000 / u64::from(rate));
            assert_eq!(samples.len(), packet.frame_count as usize);
            assert!(samples.iter().all(|sample| sample.abs_diff(8192) <= 1));
            frames += u64::from(packet.frame_count);
            packets += 1;
        }
        assert!(packets >= 2);
        assert_eq!(frames, u64::from(rate / 5));
    }
}

fn fixture_bytes(file: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../Fixtures/PublicDomainMedia")
        .join(file);
    std::fs::read(path).unwrap()
}
