use astra_media::{
    DecodeKind, DecodeOutput, DecodePolicy, DecodeProvider, DecodeProviderRegistry, DecodeRequest,
    ImageDecodeProvider, SymphoniaAudioDecodeProvider, SyntheticPlatformDecodeProvider,
};

#[test]
fn decode_provider_selection_is_profile_bound_not_load_order() {
    let mut registry = DecodeProviderRegistry::default();
    registry.register(Box::new(ImageDecodeProvider));
    registry.register(Box::new(SyntheticPlatformDecodeProvider::new(
        "platform.desktop",
        vec!["png"],
    )));

    let request = DecodeRequest {
        kind: DecodeKind::Image,
        codec: "png".to_string(),
        bytes: vec![],
        profile: "desktop-release".to_string(),
    };
    let selected = registry
        .select(&request, &DecodePolicy::desktop_release())
        .unwrap();
    assert_eq!(selected.provider_id, "platform.desktop");

    let unsupported = DecodeRequest {
        codec: "wmv".to_string(),
        ..request.clone()
    };
    assert!(registry
        .select(
            &unsupported,
            &DecodePolicy::desktop_release().without_fallback()
        )
        .is_err());

    let selected = registry
        .select(
            &request,
            &DecodePolicy::desktop_release().prefer_fallback_for_tests(),
        )
        .unwrap();
    assert_eq!(selected.provider_id, "astra.decode.image");
}

#[test]
fn symphonia_decode_provider_decodes_bounded_wav_to_cpu_pcm() {
    let provider = SymphoniaAudioDecodeProvider;
    let result = provider
        .decode(&DecodeRequest {
            kind: DecodeKind::Audio,
            codec: "wav".to_string(),
            bytes: tiny_wav(),
            profile: "desktop-release".to_string(),
        })
        .unwrap();
    assert_eq!(result.provider_id, "astra.decode.symphonia");
    match result.output {
        DecodeOutput::CpuBuffer {
            bytes,
            format,
            hash,
        } => {
            assert!(format.starts_with("pcm_s16le:8000:1"));
            assert_eq!(bytes.len(), 8);
            assert_eq!(hash, astra_core::Hash256::from_sha256(&bytes));
        }
        DecodeOutput::MediaSurfaceToken(_) => panic!("expected CPU PCM output"),
    }
}

fn tiny_wav() -> Vec<u8> {
    let samples = [0i16, 1000, -1000, 0];
    let data_len = samples.len() * 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&8000u32.to_le_bytes());
    bytes.extend_from_slice(&(8000u32 * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(data_len as u32).to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

#[cfg(feature = "ffmpeg")]
#[test]
fn ffmpeg_decode_provider_records_explicit_feature_gate() {
    if std::env::var_os("ASTRA_RUN_FFMPEG_TESTS").is_none() {
        eprintln!("skipping ffmpeg runtime probe; set ASTRA_RUN_FFMPEG_TESTS=1");
        let provider = astra_media::FfmpegDecodeProvider::new_unprobed();
        assert!(provider.capability().feature_gated);
        return;
    }
    let provider = astra_media::FfmpegDecodeProvider::probe()
        .expect("ASTRA_RUN_FFMPEG_TESTS requires the ffmpeg-system feature and native FFmpeg");
    assert!(provider
        .capability()
        .codecs
        .iter()
        .any(|codec| codec == "mp4"));
}
