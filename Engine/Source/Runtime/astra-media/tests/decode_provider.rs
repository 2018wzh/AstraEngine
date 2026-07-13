use astra_media::{
    DecodeBindingContext, DecodeKind, DecodeOutput, DecodeProvider, DecodeProviderRegistry,
    DecodeRequest, ImageDecodeProvider, SymphoniaAudioDecodeProvider,
    SyntheticPlatformDecodeProvider,
};
use serde_json::Value;

#[test]
fn decode_provider_selection_is_profile_bound_not_load_order() {
    let mut registry = DecodeProviderRegistry::default();
    registry.register(Box::new(ImageDecodeProvider)).unwrap();
    registry
        .register(Box::new(SyntheticPlatformDecodeProvider::new(
            "platform.desktop",
            vec!["png"],
        )))
        .unwrap();

    let request = DecodeRequest {
        kind: DecodeKind::Image,
        codec: "png".to_string(),
        bytes: vec![1],
        profile: "desktop-release".to_string(),
    };
    let shipping_platform =
        DecodeBindingContext::shipping("platform.desktop", "native-game", "desktop-release");
    assert!(registry.select(&request, &shipping_platform).is_err());

    let reference_platform = shipping_platform.for_reference_tests();
    let selected = registry.select(&request, &reference_platform).unwrap();
    assert_eq!(selected.provider_id, "platform.desktop");

    let image =
        DecodeBindingContext::shipping("astra.decode.image", "native-game", "desktop-release");
    assert!(registry.select(&request, &image).is_err());
    let selected = registry
        .select(&request, &image.with_declared_fallback())
        .unwrap();
    assert_eq!(selected.provider_id, "astra.decode.image");

    assert!(registry
        .select(
            &request,
            &DecodeBindingContext::shipping("missing.provider", "native-game", "desktop-release",)
        )
        .is_err());

    assert!(registry.register(Box::new(ImageDecodeProvider)).is_err());

    let mismatched_profile = DecodeRequest {
        profile: "classic".into(),
        ..request
    };
    assert!(registry
        .select(&mismatched_profile, &reference_platform)
        .is_err());
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

#[test]
fn registry_executes_only_the_explicit_provider_and_validates_output_identity() {
    let mut registry = DecodeProviderRegistry::default();
    registry
        .register(Box::new(SymphoniaAudioDecodeProvider))
        .unwrap();
    registry.register(Box::new(ImageDecodeProvider)).unwrap();
    let request = DecodeRequest {
        kind: DecodeKind::Audio,
        codec: "wav".into(),
        bytes: tiny_wav(),
        profile: "desktop-release".into(),
    };
    let binding =
        DecodeBindingContext::shipping("astra.decode.symphonia", "native-game", "desktop-release")
            .with_declared_fallback();
    let result = registry.decode(&request, &binding).unwrap();
    assert_eq!(result.provider_id, "astra.decode.symphonia");
    assert!(matches!(result.output, DecodeOutput::CpuBuffer { .. }));
}

#[test]
fn public_domain_media_manifest_matches_checked_in_assets() {
    let manifest = public_media_manifest();
    assert_eq!(manifest["license"], "CC0-1.0");
    let assets = manifest["assets"].as_array().unwrap();
    for id in ["flower_mp4", "flower_webm", "trex_roar_mp3"] {
        let asset = assets
            .iter()
            .find(|asset| asset["id"] == id)
            .unwrap_or_else(|| panic!("missing fixture asset {id}"));
        let bytes = fixture_bytes(asset["file"].as_str().unwrap());
        assert_eq!(asset["byte_size"].as_u64().unwrap(), bytes.len() as u64);
        assert_eq!(
            asset["sha256"].as_str().unwrap(),
            astra_core::Hash256::from_sha256(&bytes).to_string()
        );
        assert_eq!(asset["license"].as_str().unwrap(), "CC0-1.0");
        assert!(asset["source_url"]
            .as_str()
            .unwrap()
            .starts_with("https://"));
    }
}

#[cfg(not(feature = "ffmpeg-vcpkg"))]
#[test]
fn ffmpeg_probe_is_a_structured_blocker_when_feature_is_absent() {
    assert!(!astra_media::ffmpeg_compiled());
    match astra_media::probe_ffmpeg_provider().unwrap_err() {
        astra_media::MediaError::Diagnostics(diagnostics) => assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ASTRA_FFMPEG_FEATURE_DISABLED")),
        other => panic!("expected structured FFmpeg diagnostic, got {other:?}"),
    }
}

#[test]
fn symphonia_decode_provider_decodes_public_mp3_to_cpu_pcm() {
    let provider = SymphoniaAudioDecodeProvider;
    let result = provider
        .decode(&DecodeRequest {
            kind: DecodeKind::Audio,
            codec: "mp3".to_string(),
            bytes: fixture_bytes("t-rex-roar.mp3"),
            profile: "desktop-release".to_string(),
        })
        .unwrap();

    assert_eq!(result.provider_id, "astra.decode.symphonia");
    match result.output {
        DecodeOutput::CpuBuffer { bytes, format, .. } => {
            assert!(format.starts_with("pcm_s16le:"));
            assert!(bytes.len() > 16_000);
        }
        DecodeOutput::MediaSurfaceToken(_) => panic!("expected CPU PCM output"),
    }
}

#[cfg(windows)]
#[test]
fn windows_wmf_decode_provider_decodes_public_mp3_to_cpu_pcm() {
    let provider = astra_media::WindowsMediaFoundationDecodeProvider::probe().unwrap();
    let result = provider
        .decode(&DecodeRequest {
            kind: DecodeKind::Audio,
            codec: "mp3".to_string(),
            bytes: fixture_bytes("t-rex-roar.mp3"),
            profile: "desktop-release".to_string(),
        })
        .unwrap();

    assert_eq!(result.provider_id, "astra.decode.wmf");
    match result.output {
        DecodeOutput::CpuBuffer { bytes, format, .. } => {
            assert!(format.starts_with("pcm_s16le:"));
            assert!(bytes.len() > 16_000);
        }
        DecodeOutput::MediaSurfaceToken(_) => panic!("expected CPU PCM output"),
    }
}

#[cfg(windows)]
#[test]
fn windows_wmf_decode_provider_decodes_public_mp4_first_frame_to_bgra() {
    let provider = astra_media::WindowsMediaFoundationDecodeProvider::probe().unwrap();
    let result = provider
        .decode(&DecodeRequest {
            kind: DecodeKind::Video,
            codec: "mp4".to_string(),
            bytes: fixture_bytes("flower.mp4"),
            profile: "desktop-release".to_string(),
        })
        .unwrap();

    assert_eq!(result.provider_id, "astra.decode.wmf");
    match result.output {
        DecodeOutput::CpuBuffer {
            bytes,
            format,
            hash,
        } => {
            assert!(format.starts_with("bgra8:first_frame:"));
            assert!(bytes.len() > 320 * 180 * 4);
            assert_eq!(hash, astra_core::Hash256::from_sha256(&bytes));
        }
        DecodeOutput::MediaSurfaceToken(_) => panic!("expected CPU first-frame output"),
    }
}

#[cfg(windows)]
#[test]
fn windows_wmf_decode_provider_video_without_transform_reports_blocking_diagnostic() {
    let provider = astra_media::WindowsMediaFoundationDecodeProvider::probe().unwrap();
    let err = provider
        .decode(&DecodeRequest {
            kind: DecodeKind::Video,
            codec: "wmv".to_string(),
            bytes: b"not a video".to_vec(),
            profile: "desktop-release".to_string(),
        })
        .unwrap_err();

    match err {
        astra_media::MediaError::Diagnostics(diagnostics) => {
            assert!(diagnostics
                .iter()
                .any(|diag| diag.code == "ASTRA_WMF_DECODE"));
        }
        other => panic!("expected WMF diagnostic, got {other:?}"),
    }
}

#[cfg(target_arch = "wasm32")]
#[test]
fn webcodecs_decode_provider_returns_browser_surface_token() {
    let provider = astra_media::WebCodecsDecodeProvider;
    let capability = provider.capability();
    assert_eq!(capability.provider_id, "astra.decode.webcodecs");
    assert!(capability.codecs.iter().any(|codec| codec == "mp4"));

    let result = provider
        .decode(&DecodeRequest {
            kind: DecodeKind::Video,
            codec: "mp4".to_string(),
            bytes: vec![1, 2, 3, 4],
            profile: "web-release".to_string(),
        })
        .unwrap();

    match result.output {
        DecodeOutput::MediaSurfaceToken(token) => {
            assert_eq!(token.provider_id, "astra.decode.webcodecs");
            assert_eq!(token.format, "mp4");
        }
        DecodeOutput::CpuBuffer { .. } => panic!("expected browser-owned media token"),
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

fn public_media_manifest() -> Value {
    serde_json::from_str(include_str!(
        "../../../../Fixtures/PublicDomainMedia/manifest.json"
    ))
    .unwrap()
}

fn fixture_bytes(file: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../Fixtures/PublicDomainMedia")
        .join(file);
    std::fs::read(path).unwrap()
}

#[cfg(feature = "ffmpeg-vcpkg")]
#[test]
fn ffmpeg_decode_provider_decodes_real_audio_and_video() {
    let unavailable = astra_media::FfmpegDecodeProvider::new_unprobed()
        .decode(&DecodeRequest {
            kind: DecodeKind::Audio,
            codec: "mp3".to_string(),
            bytes: fixture_bytes("t-rex-roar.mp3"),
            profile: "desktop".to_string(),
        })
        .unwrap_err();
    assert!(unavailable.to_string().contains("explicitly probed"));

    let provider = astra_media::FfmpegDecodeProvider::probe()
        .expect("ASTRA_RUN_FFMPEG_TESTS requires the ffmpeg-vcpkg feature and native FFmpeg");
    assert!(!provider.capability().feature_gated);

    let audio = provider
        .decode(&DecodeRequest {
            kind: DecodeKind::Audio,
            codec: "mp3".to_string(),
            bytes: fixture_bytes("t-rex-roar.mp3"),
            profile: "desktop".to_string(),
        })
        .unwrap();
    assert!(matches!(
        audio.output,
        DecodeOutput::CpuBuffer { ref format, ref bytes, .. }
            if format.starts_with("pcm_s16le:") && !bytes.is_empty()
    ));

    let video = provider
        .decode(&DecodeRequest {
            kind: DecodeKind::Video,
            codec: "mp4".to_string(),
            bytes: fixture_bytes("flower.mp4"),
            profile: "desktop".to_string(),
        })
        .unwrap();
    assert!(matches!(
        video.output,
        DecodeOutput::CpuBuffer { ref format, ref bytes, .. }
            if format.starts_with("bgra8:first_frame:") && !bytes.is_empty()
    ));

    let corrupt = provider
        .decode(&DecodeRequest {
            kind: DecodeKind::Video,
            codec: "mp4".to_string(),
            bytes: b"not an mp4 container".to_vec(),
            profile: "desktop".to_string(),
        })
        .unwrap_err();
    match corrupt {
        astra_media::MediaError::Diagnostics(diagnostics) => assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ASTRA_FFMPEG_DEMUX")),
        other => panic!("expected structured FFmpeg diagnostic, got {other:?}"),
    }
}
