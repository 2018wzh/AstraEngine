use super::*;

#[test]
fn shaped_clusters_fonts_ruby_voice_and_glyph_bitmaps_reach_renderer() {
    let provider = provider();
    let mut request = request("AV cafe\u{301} office");
    request.runs[0].ruby = vec![RubySpan {
        base_range: SourceRange { start: 0, end: 2 },
        text: "pair".into(),
    }];
    request.runs[0].voice = Some(VoiceReplayRef {
        asset: "asset:/voice/opening/001".into(),
        cue: "001".into(),
    });

    let layout = provider.layout(&request).unwrap();
    assert_eq!(layout.schema, "astra.text_layout.v2");
    assert!(!layout.shaped_runs.is_empty());
    assert!(!layout.glyph_resources.is_empty());
    assert_eq!(layout.ruby_boxes.len(), 1);
    assert_eq!(layout.voice_refs.len(), 1);
    assert!(layout
        .shaped_runs
        .iter()
        .any(|run| run.role == GlyphRole::Base));
    assert!(layout
        .shaped_runs
        .iter()
        .any(|run| { run.role == GlyphRole::Ruby { span_index: 0 } }));
    assert!(layout
        .shaped_runs
        .iter()
        .flat_map(|run| &run.glyphs)
        .all(|glyph| {
            glyph.font_family == "Poppins"
                && glyph.font_hash
                    == Hash256::from_sha256(include_bytes!(
                        "../../../../../../Examples/NativeVN/Assets/Fonts/Poppins-Regular.ttf"
                    ))
                && glyph.source.start <= glyph.source.end
        }));
    assert!(layout.diagnostics.is_empty());

    let mut renderer = CpuRendererProvider
        .create(RendererCreateRequest {
            width: 320,
            height: 96,
            format: RenderTargetFormat::Rgba8Srgb,
            profile: "text-golden".into(),
        })
        .unwrap();
    let mut resource_owner = TextRenderResourceOwner::default();
    let mut commands = vec![SceneCommand::clear([8, 8, 8, 255])];
    commands.extend(
        resource_owner
            .update_layout("line.production", &layout, [255, 255, 255, 255])
            .unwrap(),
    );
    let frame = renderer.capture_frame(&commands).unwrap();
    assert!(frame
        .bytes
        .as_chunks::<4>()
        .0
        .iter()
        .any(|pixel| *pixel != [8, 8, 8, 255]));
    let redraw = resource_owner
        .update_layout("line.production", &layout, [255, 255, 255, 255])
        .unwrap();
    assert_eq!(redraw.len(), 1);
    renderer.capture_frame(&redraw).unwrap();
    let mut conflicting = layout.clone();
    let mut conflicting_pixels = conflicting.glyph_resources[0].bitmap.pixels.to_vec();
    conflicting_pixels[0] ^= 0xff;
    conflicting.glyph_resources[0].bitmap.pixels = conflicting_pixels.into();
    assert!(resource_owner
        .update_layout("line.production", &conflicting, [255; 4])
        .unwrap_err()
        .to_string()
        .contains("ASTRA_TEXT_RENDER_RESOURCE_CONFLICT"));
    assert_eq!(
        resource_owner
            .update_layout("line.production", &layout, [255; 4])
            .unwrap()
            .len(),
        1
    );
    renderer
        .capture_frame(&resource_owner.remove_layout("line.production").unwrap())
        .unwrap();

    let repeated = provider.layout(&request).unwrap();
    assert_eq!(layout.revision, repeated.revision);
    let stats = provider.cache_stats().unwrap();
    assert_eq!(stats.entries, 1);
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 1);
}

#[test]
fn measurement_reuses_the_authoritative_layout_cache_without_glyph_clone_contract() {
    let provider = provider();
    let request = request("measure this line");
    let measured = provider.measure(&request).unwrap();
    let stats_after_measure = provider.cache_stats().unwrap();
    assert_eq!(stats_after_measure.entries, 1);
    assert_eq!(stats_after_measure.misses, 1);

    let layout = provider.layout(&request).unwrap();
    assert_eq!(measured.width, layout.width);
    assert_eq!(measured.height, layout.height);
    assert_eq!(measured.revision, layout.revision);
    let stats_after_layout = provider.cache_stats().unwrap();
    assert_eq!(stats_after_layout.misses, 1);
    assert_eq!(stats_after_layout.hits, 1);
}

#[test]
fn shared_layout_reuses_the_authoritative_cached_allocation() {
    let provider = provider();
    let request = request("share this shaped line");

    let first = provider.layout_shared(&request).unwrap();
    let second = provider.layout_shared(&request).unwrap();

    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(provider.cache_stats().unwrap().entries, 1);
}

#[test]
fn validation_layouts_can_be_released_before_runtime_rendering() {
    let provider = provider();
    let request = request("cache clear validation");

    provider.layout_shared(&request).unwrap();
    let populated = provider.cache_stats().unwrap();
    assert_eq!(populated.entries, 1);
    assert_eq!(populated.misses, 1);

    provider.clear_layout_cache().unwrap();
    let cleared = provider.cache_stats().unwrap();
    assert_eq!(cleared.entries, 0);
    assert_eq!(cleared.hits, 0);
    assert_eq!(cleared.misses, 0);

    provider.layout_shared(&request).unwrap();
    let repopulated = provider.cache_stats().unwrap();
    assert_eq!(repopulated.entries, 1);
    assert_eq!(repopulated.misses, 1);
}
