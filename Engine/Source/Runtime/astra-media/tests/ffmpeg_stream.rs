#![cfg(feature = "ffmpeg-vcpkg")]

use astra_media::{
    FfmpegDecodedPacket, FfmpegPlaybackDecoder, FfmpegStreamLimits, MediaPlaybackSession,
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

fn fixture_bytes(file: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../Fixtures/PublicDomainMedia")
        .join(file);
    std::fs::read(path).unwrap()
}
