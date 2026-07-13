#![cfg(target_os = "windows")]

use astra_asset::VfsUri;
use astra_core::Hash256;
use astra_media::{
    CosmicTextLayoutProvider, FontBindingContext, FontPackageEntry, FontPackageManifest,
    LayoutConstraint, OverflowPolicy, TextDirection, TextLayoutConfig, TextLayoutProvider,
    TextLayoutRequest, TextRenderResourceOwner, TextRun, UnicodeRange, WrapPolicy,
    FONT_PACKAGE_MANIFEST_SCHEMA,
};
use astra_package::{PackageBuildRequest, PackageBuilder, PackageReader, SectionPayload};
use astra_platform::{PlatformHostFactory, PlatformHostProfile, SurfaceRequest, WindowRequest};
use astra_player_core::{
    PlatformCommandSink, PlayerHostCommand, PlayerHostCommandBatch, PlayerHostCommandExecutor,
    PlayerHostCommandResult, PlayerHostResourceId, PlayerPlatform, PlayerPresentationReport,
    PlayerPresentationRunIdentity,
};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 180;
const BACKGROUND: [u8; 4] = [12, 18, 28, 255];

struct FixtureFont {
    asset_id: &'static str,
    family: &'static str,
    file: &'static str,
    coverage: Vec<UnicodeRange>,
}

fn package() -> Vec<u8> {
    let fonts = [
        FixtureFont {
            asset_id: "asset:/font/fallback/noto-sans-sc",
            family: "Noto Sans SC",
            file: "NotoSansSC-Variable.ttf",
            coverage: vec![
                UnicodeRange {
                    start: 0x20,
                    end: 0x7e,
                },
                UnicodeRange {
                    start: 0x3000,
                    end: 0x30ff,
                },
                UnicodeRange {
                    start: 0x3400,
                    end: 0x9fff,
                },
            ],
        },
        FixtureFont {
            asset_id: "asset:/font/fallback/noto-sans-arabic",
            family: "Noto Sans Arabic",
            file: "NotoSansArabic-Variable.ttf",
            coverage: vec![
                UnicodeRange {
                    start: 0x0600,
                    end: 0x06ff,
                },
                UnicodeRange {
                    start: 0x0750,
                    end: 0x077f,
                },
                UnicodeRange {
                    start: 0x08a0,
                    end: 0x08ff,
                },
            ],
        },
        FixtureFont {
            asset_id: "asset:/font/fallback/noto-emoji",
            family: "Noto Emoji",
            file: "NotoEmoji-Variable.ttf",
            coverage: vec![
                UnicodeRange {
                    start: 0x200d,
                    end: 0x200d,
                },
                UnicodeRange {
                    start: 0x2600,
                    end: 0x27bf,
                },
                UnicodeRange {
                    start: 0xfe0f,
                    end: 0xfe0f,
                },
                UnicodeRange {
                    start: 0x1f300,
                    end: 0x1faff,
                },
            ],
        },
    ];
    let mut sections = Vec::new();
    let mut entries = Vec::new();
    let mut manifest_fonts = Vec::new();
    for (index, font) in fonts.into_iter().enumerate() {
        let bytes = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../Fixtures/PublicDomainFonts")
                .join(font.file),
        )
        .unwrap();
        let hash = Hash256::from_sha256(&bytes);
        let section_id = format!("asset.font.player.{index}");
        let uri = VfsUri::parse(&format!("package:/font/player/{index}")).unwrap();
        sections.push(SectionPayload::raw(
            section_id.clone(),
            "astra.cooked_asset.v1",
            bytes.clone(),
        ));
        entries.push(serde_json::json!({
            "vfs_uri": uri,
            "layer_id": "package.base",
            "source": { "kind": "package_section", "section_id": section_id },
            "offset": 0,
            "size": bytes.len(),
            "hash": hash,
            "codec": "raw",
            "media_kind": "font",
            "diagnostics": []
        }));
        manifest_fonts.push(FontPackageEntry {
            asset_id: font.asset_id.into(),
            uri,
            family: font.family.into(),
            face_index: 0,
            hash,
            license_id: "OFL-1.1".into(),
            subset: None,
            coverage: font.coverage,
            targets: vec!["nativevn-game".into()],
            profiles: vec!["classic".into()],
        });
    }
    let mut request =
        PackageBuildRequest::fixture("com.example.player-presentation", "classic", sections);
    request.asset_vfs_manifest = serde_json::to_vec(&serde_json::json!({
        "schema": "astra.asset_vfs_manifest.v1",
        "prefixes": [{
            "prefix": "package",
            "provider_id": "astra.vfs.package",
            "backend": "package",
            "case_policy": "case_sensitive",
            "mode": "read_only",
            "redaction": "shipping",
            "capabilities": ["vfs.backend.package"]
        }],
        "layers": [{
            "layer_id": "package.base",
            "prefix": "package",
            "priority": 0,
            "source": { "kind": "package_section", "section_id": "package.manifest" },
            "targets": ["nativevn-game"],
            "profiles": ["classic"]
        }],
        "entries": entries,
        "whiteouts": []
    }))
    .unwrap();
    request.extra_sections.push(SectionPayload::raw(
        "media.font_manifest",
        FONT_PACKAGE_MANIFEST_SCHEMA,
        serde_json::to_vec(&FontPackageManifest {
            schema: FONT_PACKAGE_MANIFEST_SCHEMA.into(),
            target: "nativevn-game".into(),
            profile: "classic".into(),
            provider_binding: "astra.vfs.package".into(),
            fonts: manifest_fonts,
        })
        .unwrap(),
    ));
    PackageBuilder::build(request).unwrap().into_bytes()
}

fn request() -> TextLayoutRequest {
    TextLayoutRequest {
        key: "windows.gpu.multiscript".into(),
        runs: vec![TextRun {
            text: "Astra 字形 日本語 العربية 😀".into(),
            language: "und".into(),
            script: None,
            direction: TextDirection::Auto,
            ruby: Vec::new(),
            voice: None,
        }],
        constraint: LayoutConstraint {
            max_width: 600.0,
            max_height: Some(140.0),
            max_lines: Some(2),
            font_size: 36.0,
            line_height: 48.0,
            wrap: WrapPolicy::WordOrGlyph,
            overflow: OverflowPolicy::Clip,
        },
        font_families: vec![
            "Noto Sans SC".into(),
            "Noto Sans Arabic".into(),
            "Noto Emoji".into(),
        ],
        features: Vec::new(),
    }
}

#[tokio::test]
async fn player_command_path_presents_packaged_layout_and_records_live_gpu_identity() {
    let golden: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../../Fixtures/PublicDomainFonts/windows-glyph-golden.json"
    ))
    .unwrap();
    let package_bytes = package();
    let package_hash = Hash256::from_sha256(&package_bytes).to_string();
    let package = PackageReader::open(&package_bytes).unwrap();
    let provider = CosmicTextLayoutProvider::from_package(
        &package,
        "media.font_manifest",
        FontBindingContext {
            target: "nativevn-game".into(),
            profile: "classic".into(),
            default_locale: "und".into(),
        },
        TextLayoutConfig::production_defaults(),
    )
    .unwrap();
    let layout = provider.layout(&request()).unwrap();
    assert_eq!(golden["layout_hash"], layout.hash.to_string());
    let mut resources = TextRenderResourceOwner::default();
    let commands = resources
        .update_layout("player.multiscript", &layout, [238, 242, 255, 255])
        .unwrap();

    let profile =
        PlatformHostProfile::windows_release("nativevn-game", "com.example.player-presentation");
    let profile_hash = profile.hash().unwrap();
    let session = astra_platform_windows::factory()
        .start(profile)
        .await
        .unwrap();
    let window = session
        .client
        .create_window(WindowRequest {
            title: "Astra Player Presentation".into(),
            width: WIDTH,
            height: HEIGHT,
            visible: true,
        })
        .await
        .unwrap();
    let surface = session
        .client
        .create_surface(SurfaceRequest {
            window,
            width: WIDTH,
            height: HEIGHT,
        })
        .await
        .unwrap();
    let logical = PlayerHostResourceId(1);
    let mut sink = PlatformCommandSink::new(session.client.clone());
    sink.bind_surface(logical, surface).unwrap();
    let mut executor = PlayerHostCommandExecutor::new(sink);
    let present = PlayerHostCommand::PresentScene {
        sequence: 1,
        surface: logical,
        width: WIDTH,
        height: HEIGHT,
        clear_rgba: BACKGROUND,
        commands: commands.clone(),
    };
    let results = executor
        .execute_batch(
            PlayerHostCommandBatch::new(vec![
                present.clone(),
                PlayerHostCommand::CaptureSurface {
                    sequence: 2,
                    surface: logical,
                },
            ])
            .unwrap(),
        )
        .await
        .unwrap();
    let PlayerHostCommandResult::Captured {
        width,
        height,
        rgba8,
        ..
    } = &results[1]
    else {
        panic!("expected live GPU capture")
    };
    let capture = astra_platform::CapturedFrame {
        width: *width,
        height: *height,
        rgba8: rgba8.clone(),
    };
    assert_eq!(
        golden["capture_hash"],
        Hash256::from_sha256(rgba8).to_string()
    );
    let font_provider_hash =
        Hash256::from_sha256(&serde_json::to_vec(&provider.identity().unwrap()).unwrap())
            .to_string();
    let report = PlayerPresentationReport::from_live_capture(
        PlayerPresentationRunIdentity {
            target: "nativevn-game".into(),
            profile: "classic".into(),
            platform: PlayerPlatform::Windows,
            package_hash,
            profile_hash,
            build_fingerprint: Hash256::from_sha256(b"player-test-build").to_string(),
            session_id: "session.windows.player-presentation".into(),
            renderer_provider: "wgpu_hardware".into(),
            presentation_path: "glyph_atlas".into(),
            font_provider_hash,
        },
        layout.hash,
        &present,
        &capture,
        BACKGROUND,
    )
    .unwrap();
    assert_eq!(report.capture_hash, golden["capture_hash"]);
    assert!(report.changed_pixels >= golden["minimum_changed_pixels"].as_u64().unwrap());

    drop(executor);
    session.client.destroy_surface(surface).await.unwrap();
    session.client.destroy_window(window).await.unwrap();
    session.client.shutdown().await.unwrap();
}
