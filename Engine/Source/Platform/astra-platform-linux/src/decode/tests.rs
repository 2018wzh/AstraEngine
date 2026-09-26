use super::*;
use astra_platform::DecodeStreamAction;
use std::time::{Duration, Instant};

fn request(bytes: Vec<u8>, sequence: u64) -> PlatformDecodeRequest {
    PlatformDecodeRequest {
        sequence,
        kind: DecodeKind::Audio,
        codec: "wav".into(),
        description: vec![],
        sample_rate: None,
        channels: None,
        coded_width: None,
        coded_height: None,
        keyframe: true,
        stream_action: DecodeStreamAction::OneShot,
        bytes: bytes.into(),
    }
}

// A complete 10 ms mono PCM file; no commercial media or device is needed.
fn wav(bits: u16) -> Vec<u8> {
    let frames = 480_u32;
    let sample_bytes = bits / 8;
    let data_len = frames * u32::from(sample_bytes);
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + data_len).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16_u32.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(48000_u32.to_le_bytes());
    bytes.extend((48000 * u32::from(sample_bytes)).to_le_bytes());
    bytes.extend(sample_bytes.to_le_bytes());
    bytes.extend(bits.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data_len.to_le_bytes());
    for index in 0..frames {
        let sample: i16 = if index % 48 < 24 { 8192 } else { -8192 };
        if bits == 24 {
            bytes.extend(&((i32::from(sample)) << 8).to_le_bytes()[..3]);
        } else {
            bytes.extend(sample.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn wav_is_audio_only_and_invalid_requests_do_not_consume_sequence() {
    assert!(supported_codec(DecodeKind::Audio, "wav"));
    assert!(!supported_codec(DecodeKind::Video, "wav"));
    assert!(!supported_codec(DecodeKind::Image, "wav"));
    let mut decoder = DecodeResource::new(DecodeKind::Image).unwrap();
    assert!(decoder.decode(request(wav(16), 1)).is_err());
    assert_eq!(decoder.next_sequence, 1);
}

#[test]
#[ignore = "requires GStreamer playback, wavparse, audioconvert and audioresample plugins"]
fn wav_decodes_complete_pcm_and_reopens_after_close() {
    for bits in [16, 24, 16] {
        let mut decoder = DecodeResource::new(DecodeKind::Audio).unwrap();
        let output = decoder.decode(request(wav(bits), 1)).unwrap();
        let DecodeOutput::AudioPcmF32 {
            sample_rate,
            channels,
            samples,
        } = output
        else {
            panic!("WAV must produce PCM");
        };
        assert_eq!((sample_rate, channels), (48000, 2));
        assert_eq!(samples.len(), 960);
        assert!(samples.iter().all(|sample| sample.is_finite()));
        assert!(samples.iter().any(|sample| sample.abs() > 0.1));
        assert!(decoder.decode(request(wav(16), 1)).is_err());
        assert!(decoder.decode(request(wav(bits), 2)).is_ok());
    }
}

#[test]
#[ignore = "requires GStreamer playback and typefind plugins"]
fn malformed_media_fails_promptly_without_consuming_sequence() {
    let mut decoder = DecodeResource::new(DecodeKind::Audio).unwrap();
    let start = Instant::now();
    let error = decoder.decode(request(vec![0x7f; 256], 1)).unwrap_err();
    assert_eq!(error.code, PlatformErrorCode::ProviderUnavailable);
    assert_eq!(
        error.fields.get("diagnostic_code").map(String::as_str),
        Some("ASTRA_LINUX_GSTREAMER_DECODE_FAILED")
    );
    assert_eq!(error.message, "GStreamer decoding failed");
    assert!(start.elapsed() < Duration::from_secs(5));
    assert_eq!(decoder.next_sequence, 1);
}

mod local_media;
mod video;
mod worker;
