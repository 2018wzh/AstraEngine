use astra_emu_sdk::video::*;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
fn source() -> Arc<[u8]> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../Engine/Fixtures/PublicDomainMedia/flower-roar.mp4"),
    )
    .unwrap()
    .into()
}
fn receive(worker: &mut VideoDecoderWorker) -> DecodeCompletion {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(value) = worker.poll().unwrap() {
            return value;
        }
        assert!(Instant::now() < deadline, "decoder operation timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn open() -> VideoDecoderWorker {
    VideoDecoderWorker::open(
        "mp4".into(),
        source(),
        FfmpegStreamLimits::default(),
        Some(FfmpegAudioOutputFormat {
            sample_rate: 48000,
            channels: 2,
        }),
    )
    .unwrap()
}
#[test]
fn worker_decodes_complete_audio_video_stream_with_bounded_requests() {
    let mut worker = open();
    assert!(worker.request_next().is_err());
    let DecodeCompletion::Opened(config) = receive(&mut worker) else {
        panic!("expected open");
    };
    assert!(config.has_audio && config.has_video);
    let (mut video, mut audio) = (0, 0);
    let mut pcm = PcmQueue::new(48000, 2, 1, 0, 48000, 64).unwrap();
    let mut expected_pcm = Vec::new();
    let mut mixed_pcm = Vec::new();
    let mut last_pts = [0; 2];
    loop {
        worker.request_next().unwrap();
        assert!(worker.request_next().is_err());
        match receive(&mut worker) {
            DecodeCompletion::Packet(Some(DecodedMediaPacket::Video { packet, bgra8 })) => {
                assert_eq!(packet.sequence, video + 1);
                assert!(packet.pts_us >= last_pts[0]);
                last_pts[0] = packet.pts_us;
                assert_eq!(
                    bgra8.len(),
                    packet.width as usize * packet.height as usize * 4
                );
                video += 1;
            }
            DecodeCompletion::Packet(Some(DecodedMediaPacket::Audio { packet, samples })) => {
                assert_eq!(packet.sequence, audio + 1);
                assert!(packet.pts_us >= last_pts[1]);
                last_pts[1] = packet.pts_us;
                assert_eq!((packet.sample_rate, packet.channels), (48000, 2));
                assert_eq!(samples.len(), packet.frame_count as usize * 2);
                expected_pcm.extend(samples.iter().map(|sample| f32::from(*sample) / 32768.0));
                pcm.push(packet, samples).unwrap();
                while !pcm.is_empty() {
                    let mut output = [0.0; 512 * 2];
                    let result = pcm.mix_into(&mut output).unwrap();
                    mixed_pcm.extend_from_slice(&output[..result.advanced_frames * 2]);
                }
                audio += 1;
            }
            DecodeCompletion::Packet(None) => break,
            _ => panic!("unexpected completion"),
        }
    }
    assert!(video > 8 && audio > 8);
    assert!(
        mixed_pcm == expected_pcm,
        "mixed={} expected={} first={:?}",
        mixed_pcm.len(),
        expected_pcm.len(),
        mixed_pcm
            .iter()
            .zip(&expected_pcm)
            .position(|(a, b)| a != b)
    );
    assert_eq!(pcm.resident_frames(), 0);
    worker.close().unwrap();
}
#[test]
fn worker_seek_replaces_generation_and_close_discards_pending_result() {
    let mut worker = open();
    let DecodeCompletion::Opened(config) = receive(&mut worker) else {
        panic!("expected open");
    };
    worker.request_next().unwrap();
    receive(&mut worker);
    worker.request_seek(config.duration_us / 2).unwrap();
    assert!(matches!(
        receive(&mut worker),
        DecodeCompletion::Seeked { generation: 2 }
    ));
    worker.request_next().unwrap();
    match receive(&mut worker) {
        DecodeCompletion::Packet(Some(DecodedMediaPacket::Video { packet, .. })) => {
            assert_eq!(packet.generation, 2)
        }
        DecodeCompletion::Packet(Some(DecodedMediaPacket::Audio { packet, .. })) => {
            assert_eq!(packet.generation, 2)
        }
        _ => panic!("seek produced no media"),
    }
    worker.request_next().unwrap();
    worker.close().unwrap();
    for _ in 0..3 {
        open().close().unwrap();
    }
}
#[test]
fn worker_open_error_is_reported_and_stops_further_requests() {
    let mut worker = VideoDecoderWorker::open(
        "mp4".into(),
        Arc::from([1u8, 2, 3]),
        FfmpegStreamLimits::default(),
        None,
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match worker.poll() {
            Err(error) => {
                assert_eq!(error.code(), "ASTRA_EMU_VIDEO_DECODE");
                break;
            }
            Ok(None) => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(Some(_)) => panic!("invalid input was accepted"),
        }
    }
    assert!(worker.request_next().is_err());
    worker.close().unwrap();
}
