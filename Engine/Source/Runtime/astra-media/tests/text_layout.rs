use astra_asset::VfsUri;
use astra_core::Hash256;
use astra_media::{
    CosmicTextLayoutProvider, FontBindingContext, FontPackageEntry, FontPackageManifest,
    LayoutConstraint, OpenTypeFeature, OverflowPolicy, PackagedFont, TextDirection,
    TextLayoutBindingIdentity, TextLayoutConfig, TextLayoutProvider, TextLayoutReplayInput,
    TextLayoutReplayLimits, TextLayoutReplaySession, TextLayoutReplaySnapshot, TextLayoutRequest,
    TextRun, UnicodeRange, WrapPolicy, FONT_PACKAGE_MANIFEST_SCHEMA,
};
use astra_package::{PackageBuildRequest, PackageBuilder, PackageReader, SectionPayload};

fn open_font_fixture(file: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../Fixtures/PublicDomainFonts")
        .join(file);
    std::fs::read(path).unwrap()
}

fn fixture_font(
    asset_id: &str,
    family: &str,
    file: &str,
    coverage: Vec<UnicodeRange>,
) -> PackagedFont {
    let bytes = open_font_fixture(file);
    PackagedFont {
        asset_id: asset_id.into(),
        family: family.into(),
        face_index: 0,
        hash: Hash256::from_sha256(&bytes),
        license_id: "OFL-1.1".into(),
        subset: None,
        coverage,
        targets: vec!["windows".into()],
        profiles: vec!["classic".into()],
        bytes,
    }
}

fn multiscript_fonts() -> Vec<PackagedFont> {
    vec![
        font(
            include_bytes!("../../../../../Examples/NativeVN/Assets/Fonts/Poppins-Regular.ttf")
                .to_vec(),
        ),
        fixture_font(
            "asset:/font/fallback/noto-sans-sc",
            "Noto Sans SC",
            "NotoSansSC-Variable.ttf",
            vec![
                UnicodeRange {
                    start: 0x3000,
                    end: 0x30ff,
                },
                UnicodeRange {
                    start: 0x3400,
                    end: 0x9fff,
                },
                UnicodeRange {
                    start: 0xff00,
                    end: 0xffef,
                },
            ],
        ),
        fixture_font(
            "asset:/font/fallback/noto-sans-arabic",
            "Noto Sans Arabic",
            "NotoSansArabic-Variable.ttf",
            vec![
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
        ),
        fixture_font(
            "asset:/font/fallback/noto-emoji",
            "Noto Emoji",
            "NotoEmoji-Variable.ttf",
            vec![
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
        ),
    ]
}

fn font(bytes: Vec<u8>) -> PackagedFont {
    PackagedFont {
        asset_id: "asset:/font/ui/poppins-regular".into(),
        family: "Poppins".into(),
        face_index: 0,
        hash: Hash256::from_sha256(&bytes),
        license_id: "OFL-1.1".into(),
        subset: None,
        coverage: vec![
            UnicodeRange {
                start: 0,
                end: 0x036f,
            },
            UnicodeRange {
                start: 0x2000,
                end: 0x206f,
            },
        ],
        targets: vec!["windows".into()],
        profiles: vec!["classic".into()],
        bytes,
    }
}

fn provider() -> CosmicTextLayoutProvider {
    CosmicTextLayoutProvider::new(
        FontBindingContext {
            target: "windows".into(),
            profile: "classic".into(),
            default_locale: "en-US".into(),
        },
        vec![font(
            include_bytes!("../../../../../Examples/NativeVN/Assets/Fonts/Poppins-Regular.ttf")
                .to_vec(),
        )],
        TextLayoutConfig::production_defaults(),
    )
    .unwrap()
}

fn request(text: &str) -> TextLayoutRequest {
    TextLayoutRequest {
        key: "line.production".into(),
        runs: vec![TextRun {
            text: text.into(),
            language: "en-US".into(),
            script: Some("Latn".into()),
            direction: TextDirection::LeftToRight,
            ruby: Vec::new(),
            voice: None,
        }],
        constraint: LayoutConstraint {
            max_width: 240.0,
            max_height: None,
            max_lines: None,
            font_size: 24.0,
            line_height: 32.0,
            wrap: WrapPolicy::WordOrGlyph,
            overflow: OverflowPolicy::Visible,
        },
        font_families: vec!["Poppins".into()],
        features: vec![
            OpenTypeFeature {
                tag: "kern".into(),
                value: 1,
            },
            OpenTypeFeature {
                tag: "liga".into(),
                value: 1,
            },
        ],
    }
}

fn replay_binding(provider: &CosmicTextLayoutProvider) -> TextLayoutBindingIdentity {
    TextLayoutBindingIdentity {
        provider_id: "astra.media.cosmic_text".into(),
        provider_fingerprint: Hash256::from_sha256(b"cosmic-text-test-provider-v1"),
        package_hash: Hash256::from_sha256(b"text-layout-test-package-v1"),
        build_fingerprint: Hash256::from_sha256(b"text-layout-test-build-v1"),
        session_id: "session.text-layout-test".into(),
        provider: provider.identity().unwrap(),
    }
}

#[test]
fn verified_package_vfs_is_the_font_database_authority() {
    let font_bytes =
        include_bytes!("../../../../../Examples/NativeVN/Assets/Fonts/Poppins-Regular.ttf")
            .to_vec();
    let font_hash = Hash256::from_sha256(&font_bytes);
    let font_section =
        SectionPayload::raw("asset.font.ui", "astra.cooked_asset.v1", font_bytes.clone());
    let mut build =
        PackageBuildRequest::fixture("com.example.font-authority", "classic", vec![font_section]);
    build.asset_vfs_manifest = serde_json::to_vec(&serde_json::json!({
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
            "targets": ["native-smoke-game"],
            "profiles": ["classic"]
        }],
        "entries": [{
            "vfs_uri": "package:/asset/font/ui",
            "layer_id": "package.base",
            "source": { "kind": "package_section", "section_id": "asset.font.ui" },
            "offset": 0,
            "size": font_bytes.len(),
            "hash": font_hash,
            "codec": "raw",
            "media_kind": "font",
            "diagnostics": []
        }],
        "whiteouts": []
    }))
    .unwrap();
    let manifest = FontPackageManifest {
        schema: FONT_PACKAGE_MANIFEST_SCHEMA.into(),
        target: "native-smoke-game".into(),
        profile: "classic".into(),
        provider_binding: "astra.vfs.package".into(),
        fonts: vec![FontPackageEntry {
            asset_id: "asset:/font/ui/poppins-regular".into(),
            uri: VfsUri::parse("package:/asset/font/ui").unwrap(),
            family: "Poppins".into(),
            face_index: 0,
            hash: font_hash,
            license_id: "OFL-1.1".into(),
            subset: None,
            coverage: vec![UnicodeRange {
                start: 0,
                end: 0x036f,
            }],
            targets: vec!["native-smoke-game".into()],
            profiles: vec!["classic".into()],
        }],
    };
    build.extra_sections.push(SectionPayload::raw(
        "media.font_manifest",
        FONT_PACKAGE_MANIFEST_SCHEMA,
        serde_json::to_vec(&manifest).unwrap(),
    ));
    let blob = PackageBuilder::build(build).unwrap();
    let package = PackageReader::open(blob.as_bytes()).unwrap();
    let provider = astra_media::text_layout_from_package(
        &package,
        "media.font_manifest",
        FontBindingContext {
            target: "native-smoke-game".into(),
            profile: "classic".into(),
            default_locale: "en-US".into(),
        },
        TextLayoutConfig::production_defaults(),
    )
    .unwrap();
    assert!(!provider
        .layout(&request("package font"))
        .unwrap()
        .glyph_resources
        .is_empty());

    let wrong_context = astra_media::text_layout_from_package(
        &package,
        "media.font_manifest",
        FontBindingContext {
            target: "other-game".into(),
            profile: "classic".into(),
            default_locale: "en-US".into(),
        },
        TextLayoutConfig::production_defaults(),
    )
    .err()
    .unwrap();
    assert!(wrong_context
        .to_string()
        .contains("ASTRA_TEXT_PACKAGE_MANIFEST_IDENTITY"));
}

#[test]
fn multiscript_fallback_database_is_loaded_from_verified_package_sections() {
    let mut fonts = multiscript_fonts();
    for font in &mut fonts {
        font.targets = vec!["native-smoke-game".into()];
    }
    let mut sections = Vec::new();
    let mut entries = Vec::new();
    let mut manifest_fonts = Vec::new();
    for (index, font) in fonts.iter().enumerate() {
        let section_id = format!("asset.font.multiscript.{index}");
        let uri = VfsUri::parse(&format!("package:/font/multiscript/{index}")).unwrap();
        sections.push(SectionPayload::raw(
            section_id.clone(),
            "astra.cooked_asset.v1",
            font.bytes.clone(),
        ));
        entries.push(serde_json::json!({
            "vfs_uri": uri,
            "layer_id": "package.base",
            "source": { "kind": "package_section", "section_id": section_id },
            "offset": 0,
            "size": font.bytes.len(),
            "hash": font.hash,
            "codec": "raw",
            "media_kind": "font",
            "diagnostics": []
        }));
        manifest_fonts.push(FontPackageEntry {
            asset_id: font.asset_id.clone(),
            uri,
            family: font.family.clone(),
            face_index: font.face_index,
            hash: font.hash,
            license_id: font.license_id.clone(),
            subset: font.subset.clone(),
            coverage: font.coverage.clone(),
            targets: font.targets.clone(),
            profiles: font.profiles.clone(),
        });
    }
    let mut build =
        PackageBuildRequest::fixture("com.example.multiscript-fonts", "classic", sections);
    build.asset_vfs_manifest = serde_json::to_vec(&serde_json::json!({
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
            "targets": ["native-smoke-game"],
            "profiles": ["classic"]
        }],
        "entries": entries,
        "whiteouts": []
    }))
    .unwrap();
    let manifest = FontPackageManifest {
        schema: FONT_PACKAGE_MANIFEST_SCHEMA.into(),
        target: "native-smoke-game".into(),
        profile: "classic".into(),
        provider_binding: "astra.vfs.package".into(),
        fonts: manifest_fonts,
    };
    build.extra_sections.push(SectionPayload::raw(
        "media.font_manifest",
        FONT_PACKAGE_MANIFEST_SCHEMA,
        serde_json::to_vec(&manifest).unwrap(),
    ));
    let blob = PackageBuilder::build(build).unwrap();
    let package = PackageReader::open(blob.as_bytes()).unwrap();
    let provider = astra_media::text_layout_from_package(
        &package,
        "media.font_manifest",
        FontBindingContext {
            target: "native-smoke-game".into(),
            profile: "classic".into(),
            default_locale: "und".into(),
        },
        TextLayoutConfig::production_defaults(),
    )
    .unwrap();
    let mut mixed = request("A中ا😀");
    mixed.runs[0].language = "und".into();
    mixed.runs[0].script = None;
    mixed.runs[0].direction = TextDirection::Auto;
    mixed.font_families = vec![
        "Poppins".into(),
        "Noto Sans SC".into(),
        "Noto Sans Arabic".into(),
        "Noto Emoji".into(),
    ];
    let layout = provider.layout(&mixed).unwrap();
    let families = layout
        .shaped_runs
        .iter()
        .map(|run| run.font_family.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(families.len(), 4);
    assert_eq!(layout.revision, provider.layout(&mixed).unwrap().revision);
}

#[test]
fn layout_snapshot_restore_and_provider_free_replay_are_deterministic() {
    let provider = provider();
    let binding = replay_binding(&provider);
    let limits = TextLayoutReplayLimits::production_defaults();
    let first = request("first e\u{301} line");
    let second = request("second deterministic line");

    let mut uninterrupted = TextLayoutReplaySession::live(binding.clone(), limits).unwrap();
    let first_layout = uninterrupted.record_live(&provider, &first).unwrap();
    let checkpoint = uninterrupted.snapshot().unwrap();
    let second_layout = uninterrupted.record_live(&provider, &second).unwrap();
    let uninterrupted_hash = uninterrupted.transcript_hash();

    let mut restored =
        TextLayoutReplaySession::restore_live(&checkpoint, &binding, limits.max_snapshot_bytes)
            .unwrap();
    assert_eq!(
        restored.record_live(&provider, &second).unwrap(),
        second_layout
    );
    assert_eq!(restored.transcript_hash(), uninterrupted_hash);

    let transcript = restored.snapshot().unwrap();
    let mut replay =
        TextLayoutReplaySession::restore_replay(&transcript, &binding, limits.max_snapshot_bytes)
            .unwrap();
    let first_request_hash = astra_media::text_layout_replay_request_hash(&first).unwrap();
    let second_request_hash = astra_media::text_layout_replay_request_hash(&second).unwrap();
    assert_eq!(
        replay
            .replay_next(TextLayoutReplayInput {
                sequence: 1,
                request_hash: first_request_hash,
            })
            .unwrap(),
        first_layout
    );
    let replay_checkpoint = replay.snapshot().unwrap();
    let mut replay = TextLayoutReplaySession::restore_replay(
        &replay_checkpoint,
        &binding,
        limits.max_snapshot_bytes,
    )
    .unwrap();
    assert_eq!(replay.replayed_layouts(), 1);
    assert_eq!(
        replay
            .replay_next(TextLayoutReplayInput {
                sequence: 2,
                request_hash: second_request_hash,
            })
            .unwrap(),
        second_layout
    );
    assert_eq!(replay.replayed_layouts(), 2);
    assert!(replay
        .replay_next(TextLayoutReplayInput {
            sequence: 3,
            request_hash: second_request_hash,
        })
        .is_err());
}

#[test]
fn layout_replay_blocks_request_provider_and_payload_drift_without_advancing() {
    let provider = provider();
    let binding = replay_binding(&provider);
    let limits = TextLayoutReplayLimits::production_defaults();
    let request = request("recorded layout");
    let request_hash = astra_media::text_layout_replay_request_hash(&request).unwrap();
    let mut live = TextLayoutReplaySession::live(binding.clone(), limits).unwrap();
    let expected = live.record_live(&provider, &request).unwrap();
    let bytes = live.snapshot().unwrap();

    let mut replay =
        TextLayoutReplaySession::restore_replay(&bytes, &binding, limits.max_snapshot_bytes)
            .unwrap();
    let error = replay
        .replay_next(TextLayoutReplayInput {
            sequence: 1,
            request_hash: Hash256::from_sha256(b"different request"),
        })
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_TEXT_REPLAY_REQUEST_DRIFT"));
    assert_eq!(replay.replayed_layouts(), 0);
    assert_eq!(
        replay
            .replay_next(TextLayoutReplayInput {
                sequence: 1,
                request_hash,
            })
            .unwrap(),
        expected
    );

    let mut drifted_binding = binding.clone();
    drifted_binding.provider_fingerprint = Hash256::from_sha256(b"changed provider");
    let error =
        TextLayoutReplaySession::restore_live(&bytes, &drifted_binding, limits.max_snapshot_bytes)
            .err()
            .unwrap();
    assert!(error.to_string().contains("ASTRA_TEXT_PROVIDER_DRIFT"));

    let mut decoded: TextLayoutReplaySnapshot = postcard::from_bytes(&bytes).unwrap();
    let mut tampered_pixels = decoded.records[0].layout.glyph_resources[0]
        .bitmap
        .pixels
        .to_vec();
    tampered_pixels[0] ^= 0xff;
    decoded.records[0].layout.glyph_resources[0].bitmap.pixels = tampered_pixels.into();
    let tampered = postcard::to_allocvec(&decoded).unwrap();
    let error =
        TextLayoutReplaySession::restore_replay(&tampered, &binding, limits.max_snapshot_bytes)
            .err()
            .unwrap();
    assert!(error.to_string().contains("ASTRA_TEXT_REPLAY_RECORD"));
}

#[test]
fn layout_replay_enforces_record_and_snapshot_budgets_transactionally() {
    let provider = provider();
    let binding = replay_binding(&provider);
    let limits = TextLayoutReplayLimits {
        max_records: 1,
        max_snapshot_bytes: 64 * 1024 * 1024,
        max_glyph_resources_per_record: 65_536,
        max_glyph_bytes_per_record: 64 * 1024 * 1024,
    };
    let mut live = TextLayoutReplaySession::live(binding, limits).unwrap();
    live.record_live(&provider, &request("bounded record"))
        .unwrap();
    let before = live.transcript_hash();
    let error = live
        .record_live(&provider, &request("overflow record"))
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_TEXT_REPLAY_RECORD_BUDGET"));
    assert_eq!(live.recorded_layouts(), 1);
    assert_eq!(live.transcript_hash(), before);
}
