use astra_media::{
    DecodeKind, DecodeOutput, DecodePolicy, DecodeProvider, DecodeProviderRegistry, DecodeRequest,
    ImageDecodeProvider, SymphoniaAudioDecodeProvider, SyntheticPlatformDecodeProvider,
};
use serde_json::Value;

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
        .expect("ASTRA_RUN_FFMPEG_TESTS requires the ffmpeg-vcpkg feature and native FFmpeg");
    assert!(provider
        .capability()
        .codecs
        .iter()
        .any(|codec| codec == "mp4"));
}
