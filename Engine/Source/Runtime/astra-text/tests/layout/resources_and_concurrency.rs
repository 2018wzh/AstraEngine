use super::*;

#[test]
fn frame_resource_journal_is_incremental_shared_and_transactional() {
    let provider = provider();
    let layout = provider.layout(&request("shared frame glyphs")).unwrap();
    let mut owner = TextRenderResourceOwner::default();
    let initial = owner
        .update_frame(
            &[
                TextRenderLayoutUpdate {
                    layout_id: "frame.a",
                    layout: &layout,
                    shared_layout: None,
                    rgba: [255; 4],
                    translation: (0, 0),
                },
                TextRenderLayoutUpdate {
                    layout_id: "frame.b",
                    layout: &layout,
                    shared_layout: None,
                    rgba: [255; 4],
                    translation: (0, 0),
                },
            ],
            &[],
        )
        .unwrap();
    assert_eq!(initial.layouts.len(), 2);
    assert_eq!(
        initial
            .lifecycle
            .iter()
            .filter(|command| matches!(command, SceneCommand::UploadGlyph { .. }))
            .count(),
        layout.glyph_resources.len()
    );

    let stable = owner
        .update_frame(
            &[
                TextRenderLayoutUpdate {
                    layout_id: "frame.a",
                    layout: &layout,
                    shared_layout: None,
                    rgba: [255; 4],
                    translation: (0, 0),
                },
                TextRenderLayoutUpdate {
                    layout_id: "frame.b",
                    layout: &layout,
                    shared_layout: None,
                    rgba: [255; 4],
                    translation: (0, 0),
                },
            ],
            &[],
        )
        .unwrap();
    assert!(stable.lifecycle.is_empty());

    let mut conflicting = layout.clone();
    let mut conflicting_pixels = conflicting.glyph_resources[0].bitmap.pixels.to_vec();
    conflicting_pixels[0] ^= 0xff;
    conflicting.glyph_resources[0].bitmap.pixels = conflicting_pixels.into();
    let error = owner
        .update_frame(
            &[TextRenderLayoutUpdate {
                layout_id: "frame.a",
                layout: &conflicting,
                shared_layout: None,
                rgba: [255; 4],
                translation: (0, 0),
            }],
            &["frame.b"],
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_TEXT_RENDER_RESOURCE_CONFLICT"));

    let after_failure = owner
        .update_frame(
            &[
                TextRenderLayoutUpdate {
                    layout_id: "frame.a",
                    layout: &layout,
                    shared_layout: None,
                    rgba: [255; 4],
                    translation: (0, 0),
                },
                TextRenderLayoutUpdate {
                    layout_id: "frame.b",
                    layout: &layout,
                    shared_layout: None,
                    rgba: [255; 4],
                    translation: (0, 0),
                },
            ],
            &[],
        )
        .unwrap();
    assert!(after_failure.lifecycle.is_empty());

    let removed = owner.update_frame(&[], &["frame.a", "frame.b"]).unwrap();
    assert_eq!(
        removed
            .lifecycle
            .iter()
            .filter(|command| matches!(command, SceneCommand::ReleaseResource { .. }))
            .count(),
        layout.glyph_resources.len()
    );
}

#[test]
fn retained_glyph_cache_reuses_resources_and_evicts_under_pressure() {
    let provider = provider();
    let layout = provider
        .layout(&request("retained glyph resources"))
        .unwrap();
    let mut owner = TextRenderResourceOwner::with_retained_glyph_cache(4_096, 1024 * 1024).unwrap();
    let initial = owner.update_layout("retained", &layout, [255; 4]).unwrap();
    let upload_count = initial
        .iter()
        .filter(|command| matches!(command, SceneCommand::UploadGlyph { .. }))
        .count();
    assert_eq!(upload_count, layout.glyph_resources.len());

    let removed = owner.remove_layout("retained").unwrap();
    assert!(removed.is_empty());
    let reused = owner.update_layout("retained", &layout, [255; 4]).unwrap();
    assert!(!reused
        .iter()
        .any(|command| matches!(command, SceneCommand::UploadGlyph { .. })));
    owner.remove_layout("retained").unwrap();
    assert_eq!(
        owner
            .shutdown()
            .iter()
            .filter(|command| matches!(command, SceneCommand::ReleaseResource { .. }))
            .count(),
        layout.glyph_resources.len()
    );

    let mut constrained = TextRenderResourceOwner::with_retained_glyph_cache(1, 1).unwrap();
    constrained
        .update_layout("constrained", &layout, [255; 4])
        .unwrap();
    let evicted = constrained.remove_layout("constrained").unwrap();
    assert_eq!(
        evicted
            .iter()
            .filter(|command| matches!(command, SceneCommand::ReleaseResource { .. }))
            .count(),
        layout.glyph_resources.len()
    );
    assert!(TextRenderResourceOwner::with_retained_glyph_cache(0, 1).is_err());
    assert!(TextRenderResourceOwner::with_retained_glyph_cache(1, 0).is_err());
}

#[test]
fn text_layout_single_flight_and_worker_pool_are_bounded_and_parallel() {
    let provider = Arc::new(provider());
    let barrier = Arc::new(Barrier::new(8));
    let handles = (0..8)
        .map(|_| {
            let provider = Arc::clone(&provider);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                provider.layout_shared(&request("single flight")).unwrap()
            })
        })
        .collect::<Vec<_>>();
    let layouts = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert!(layouts
        .iter()
        .all(|layout| Arc::ptr_eq(layout, &layouts[0])));
    let cache = provider.cache_stats().unwrap();
    assert_eq!(cache.misses, 1);
    assert_eq!(cache.hits, 7);

    provider.clear_layout_cache().unwrap();
    let worker_count = provider.concurrency_stats().unwrap().worker_count;
    let barrier = Arc::new(Barrier::new(worker_count));
    let handles = (0..worker_count)
        .map(|index| {
            let provider = Arc::clone(&provider);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let text = format!("worker-{index} {}", "parallel shaping ".repeat(4_000));
                let request = request(&text);
                barrier.wait();
                provider.layout_shared(&request).unwrap()
            })
        })
        .collect::<Vec<_>>();
    for handle in handles {
        handle.join().unwrap();
    }
    let concurrency = provider.concurrency_stats().unwrap();
    assert_eq!(concurrency.active_workers, 0);
    assert_eq!(concurrency.in_flight_requests, 0);
    if concurrency.worker_count > 1 {
        assert!(
            concurrency.peak_active_workers > 1,
            "distinct layout misses must execute on more than one worker"
        );
    }
}

#[test]
fn overlapping_cjk_fallback_honors_the_explicit_family_order() {
    let jp_bytes =
        include_bytes!("../../../../../../Examples/NativeVN/Assets/Fonts/NotoSansJP-Variable.ttf")
            .to_vec();
    let jp = PackagedFont {
        asset_id: "asset:/font/fallback/noto-sans-jp".into(),
        family: "Noto Sans JP".into(),
        face_index: 0,
        hash: Hash256::from_sha256(&jp_bytes),
        license_id: "OFL-1.1".into(),
        subset: None,
        coverage: vec![
            UnicodeRange {
                start: 32,
                end: 126,
            },
            UnicodeRange {
                start: 0x3000,
                end: 0x9fff,
            },
        ],
        targets: vec!["windows".into()],
        profiles: vec!["classic".into()],
        bytes: jp_bytes,
    };
    let sc = fixture_font(
        "asset:/font/fallback/noto-sans-sc",
        "Noto Sans SC",
        "NotoSansSC-Variable.ttf",
        vec![
            UnicodeRange {
                start: 32,
                end: 126,
            },
            UnicodeRange {
                start: 0x3000,
                end: 0x9fff,
            },
        ],
    );
    let provider = CosmicTextLayoutProvider::new(
        FontBindingContext {
            target: "windows".into(),
            profile: "classic".into(),
            default_locale: "ja".into(),
        },
        vec![jp, sc],
        TextLayoutConfig::production_defaults(),
    )
    .unwrap();
    let sample = "CONFIG SAVE LOAD EXIT　設定文字を隠す通常早送り音声ありなし";
    for character in sample.chars() {
        let mut request = request(&character.to_string());
        request.runs[0].language = "ja".into();
        request.runs[0].script = Some("Jpan".into());
        request.font_families = vec!["Noto Sans JP".into(), "Noto Sans SC".into()];
        provider.layout(&request).unwrap_or_else(|error| {
            panic!(
                "CJK fallback failed for U+{:04X}: {error}",
                character as u32
            )
        });
    }
    let mut sample_request = request(sample);
    sample_request.runs[0].language = "ja".into();
    sample_request.runs[0].script = Some("Jpan".into());
    sample_request.font_families = vec!["Noto Sans JP".into(), "Noto Sans SC".into()];
    provider.layout(&sample_request).unwrap();

    let mut short_request = request("設定");
    short_request.runs[0].language = "ja".into();
    short_request.runs[0].script = Some("Jpan".into());
    short_request.font_families = vec!["Noto Sans JP".into(), "Noto Sans SC".into()];
    provider.layout(&short_request).unwrap();
}
