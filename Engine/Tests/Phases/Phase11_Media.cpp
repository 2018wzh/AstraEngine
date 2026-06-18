TEST_CASE("Media foundation extracts presentation commands and produces stable headless hashes") {
    Astra::Core::DiagnosticSink diagnostics;
    auto background = Astra::Asset::ParseAssetUri("native:/Backgrounds/Room");
    auto character = Astra::Asset::ParseAssetUri("native:/Characters/Alice/Normal");
    auto voice = Astra::Asset::ParseAssetUri("native:/Voice/Alice/opening_001");
    auto filter_id = Astra::Asset::ParseAssetUri("native:/Filters/soft_vn");
    REQUIRE(background);
    REQUIRE(character);
    REQUIRE(voice);
    REQUIRE(filter_id);

    Astra::Media::FilterProfile profile;
    profile.id = filter_id.Value();
    profile.passes.push_back({"bg_blur",
                              "astra.filter.gaussian_blur",
                              Astra::Media::FilterTarget::Background,
                              {{"radius", 2}}});
    profile.passes.push_back({"character_line",
                              "astra.filter.line_enhance",
                              Astra::Media::FilterTarget::Character,
                              {{"strength", 0.4}}});
    profile.passes.push_back({"final_grade",
                              "astra.filter.color_grade",
                              Astra::Media::FilterTarget::Final,
                              {{"preset", "warm"}}});
    REQUIRE(Astra::Media::ValidateFilterProfile(profile, diagnostics));

    std::vector<Astra::Media::PresentationCommand> commands;
    Astra::Media::PresentationCommand background_draw;
    background_draw.kind = Astra::Media::PresentationCommandKind::Sprite;
    background_draw.command_id = "draw.background";
    background_draw.frame_index = 2;
    background_draw.layer = "background";
    background_draw.asset = background.Value();
    commands.push_back(std::move(background_draw));

    Astra::Media::PresentationCommand character_draw;
    character_draw.kind = Astra::Media::PresentationCommandKind::Sprite;
    character_draw.command_id = "draw.character";
    character_draw.frame_index = 2;
    character_draw.layer = "character";
    character_draw.order = 100;
    character_draw.asset = character.Value();
    commands.push_back(std::move(character_draw));

    Astra::Media::PresentationCommand text;
    text.kind = Astra::Media::PresentationCommandKind::Text;
    text.command_id = "text.dialogue";
    text.frame_index = 2;
    text.layer = "text";
    text.order = 200;
    text.text = "Hello from Phase 3.";
    text.locale = "en-US";
    commands.push_back(std::move(text));

    Astra::Media::PresentationCommand audio;
    audio.kind = Astra::Media::PresentationCommandKind::Audio;
    audio.command_id = "audio.voice";
    audio.frame_index = 2;
    audio.asset = voice.Value();
    audio.bus = "voice";
    audio.payload = {{"kind", "play"}, {"volume", 0.75}};
    commands.push_back(std::move(audio));

    auto graph = Astra::Media::ExtractRenderGraph(commands, &profile, diagnostics);
    REQUIRE(graph.draws.size() == 2);
    REQUIRE(graph.text_requests.size() == 1);
    REQUIRE(graph.audio_commands.size() == 1);
    REQUIRE(graph.filter_applications.size() == 3);

    auto renderer = Astra::Media::CreateHeadlessRenderer2D();
    renderer->Submit(graph, diagnostics);
    const auto first = renderer->Capture();
    renderer->Submit(graph, diagnostics);
    const auto second = renderer->Capture();
    REQUIRE(first.render_hash == second.render_hash);
    REQUIRE(first.text_hash == second.text_hash);
    REQUIRE(first.audio_hash == second.audio_hash);
    REQUIRE(first.filter_hash == second.filter_hash);
    REQUIRE_FALSE(first.filter_hash.empty());
    REQUIRE(Astra::Media::ToJson(first)["commands"]["filter_applications"].size() == 3);

    const auto png_1x1_rgba = TestPng1x1Rgba();
    auto image = Astra::Media::InspectImageBytes(png_1x1_rgba, diagnostics);
    REQUIRE(image);
    REQUIRE(image.Value().format == "png");
    REQUIRE(image.Value().width == 1);
    REQUIRE(image.Value().height == 1);
    REQUIRE(image.Value().has_alpha);
    REQUIRE(Astra::Media::ToJson(image.Value())["decoded_by"] == "libpng");
    const auto fixture_png = ReadFixtureBytes(std::filesystem::path(ASTRA_SOURCE_ROOT) /
                                              "Samples/NativeVN/Content/Backgrounds/Room.png");
    REQUIRE_FALSE(fixture_png.empty());
    auto rgba = Astra::Media::DecodeImageRgbaBytes(fixture_png, diagnostics);
    REQUIRE(rgba);
    REQUIRE(rgba.Value().width > 1);
    REQUIRE(rgba.Value().height > 1);
    REQUIRE(rgba.Value().pixels.size() ==
            static_cast<std::size_t>(rgba.Value().width) * rgba.Value().height * 4);
    REQUIRE(rgba.Value().decoded_by == "libpng");
    const auto font_payload =
        ReadFixtureBytes(std::filesystem::path(ASTRA_SOURCE_ROOT) /
                         "Samples/NativeVN/Content/Fonts/sourcehanserif-medium.otf");
    REQUIRE_FALSE(font_payload.empty());
    auto rasterized_text =
        Astra::Media::RasterizeTextRgbaBytes(font_payload, "Aya route text 確認", 28, diagnostics);
    REQUIRE(rasterized_text);
    REQUIRE(rasterized_text.Value().glyph_count > 0);
    REQUIRE(rasterized_text.Value().width > 0);
    REQUIRE(rasterized_text.Value().height > 0);
    REQUIRE(rasterized_text.Value().pixels.size() ==
            static_cast<std::size_t>(rasterized_text.Value().width) *
                rasterized_text.Value().height * 4);
    REQUIRE(rasterized_text.Value().rasterized_by == "freetype");
    const auto audio_payload = ReadFixtureBytes(std::filesystem::path(ASTRA_SOURCE_ROOT) /
                                                "Samples/NativeVN/Content/Music/opening_theme.ogg");
    REQUIRE_FALSE(audio_payload.empty());
    auto audio_decode = Astra::Media::DecodeAudioBytes(audio_payload, diagnostics);
    REQUIRE(audio_decode);
    REQUIRE(audio_decode.Value().format == "ogg");
    REQUIRE(audio_decode.Value().channels > 0);
    REQUIRE(audio_decode.Value().sample_rate > 0);
    REQUIRE(audio_decode.Value().pcm_frame_count > 0);
    REQUIRE((audio_decode.Value().decoded_by.starts_with("ffmpeg/") ||
             audio_decode.Value().decoded_by == "libvorbisfile" ||
             audio_decode.Value().decoded_by == "miniaudio"));
    const auto artemis_bgm_payload = ReadFixtureBytes(
        std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/NativeVN/Content/Sfx/confirm.ogg");
    REQUIRE_FALSE(artemis_bgm_payload.empty());
    auto artemis_bgm_decode = Astra::Media::DecodeAudioBytes(artemis_bgm_payload, diagnostics);
    if (artemis_bgm_decode) {
        REQUIRE(artemis_bgm_decode.Value().channels > 0);
        REQUIRE(artemis_bgm_decode.Value().sample_rate > 0);
        REQUIRE(artemis_bgm_decode.Value().pcm_frame_count > 0);
        REQUIRE(artemis_bgm_decode.Value().decoded_by.starts_with("ffmpeg/"));
    }
}

TEST_CASE("Media provider descriptors and release gate validate foundation slots") {
    const auto capabilities = Astra::Media::ProbeMediaBackendCapabilities();
    const auto capabilities_json = Astra::Media::ToJson(capabilities);
    REQUIRE(capabilities_json["schema"] == "astra.media.backend_capabilities.v1");
    REQUIRE(capabilities_json["libraries"].size() >= 6);
    REQUIRE(capabilities_json["libraries"][0]["id"] == "sdl3");
    REQUIRE(capabilities_json["renderer2d_ready"].is_boolean());
    REQUIRE(capabilities_json["ui_text_raster_ready"].is_boolean());

    auto providers = Astra::Media::FoundationMediaProviders();
    REQUIRE(providers.size() == 3);

    Astra::Core::DiagnosticSink diagnostics;
    for (const auto& provider : providers) {
        REQUIRE(Astra::Media::ValidateMediaProviderDescriptor(provider, diagnostics));
    }
    REQUIRE_FALSE(diagnostics.HasBlocking());

    auto filter_id = Astra::Asset::ParseAssetUri("native:/Filters/release_gate");
    REQUIRE(filter_id);
    Astra::Media::FilterProfile profile;
    profile.id = filter_id.Value();
    profile.passes.push_back({"bg_soften",
                              "astra.filter.foundation_hash",
                              Astra::Media::FilterTarget::Background,
                              {{"strength", 0.25}}});
    profile.passes.push_back({"final_grade",
                              "astra.filter.foundation_hash",
                              Astra::Media::FilterTarget::Final,
                              {{"grade", "warm"}}});

    Astra::Media::MediaReleaseGateRequest request;
    request.providers = providers;
    request.filter_profile = profile;
    for (const auto& provider : providers) {
        request.selected_providers[provider.slot_id] = provider.provider_id;
    }
    auto report = Astra::Media::ValidateMediaReleaseGate(request, diagnostics);
    REQUIRE(report);
    REQUIRE(report.Value().selected_providers.size() == 3);
    REQUIRE(report.Value().filter_applications.size() == 2);
    REQUIRE(Astra::Media::ToJson(report.Value())["schema"] == Astra::Media::MediaReleaseGateSchema);

    auto production_request = request;
    production_request.providers = Astra::Media::ProductionMediaProviders();
    production_request.selected_providers.clear();
    production_request.require_available_backends = true;
    for (const auto& provider : production_request.providers) {
        production_request.selected_providers[provider.slot_id] = provider.provider_id;
    }
    Astra::Core::DiagnosticSink production_diagnostics;
    auto production_report =
        Astra::Media::ValidateMediaReleaseGate(production_request, production_diagnostics);
    if (capabilities.renderer2d_ready && capabilities.ui_text_raster_ready) {
        REQUIRE(production_report);
    } else {
        REQUIRE_FALSE(production_report);
        REQUIRE(production_diagnostics.HasBlocking());
    }

    std::vector<Astra::Media::PresentationCommand> commands;
    Astra::Media::PresentationCommand draw;
    draw.kind = Astra::Media::PresentationCommandKind::Sprite;
    draw.command_id = "draw.release_gate";
    draw.frame_index = 3;
    draw.layer = "background";
    draw.asset = Astra::Asset::ParseAssetUri("native:/Backgrounds/Room").Value();
    commands.push_back(std::move(draw));
    auto graph = Astra::Media::ExtractRenderGraph(commands, &profile, diagnostics);
    auto renderer = Astra::Media::CreateHeadlessRenderer2D();
    renderer->Submit(graph, diagnostics);
    auto capture = renderer->Capture();
    REQUIRE_FALSE(capture.render_hash.empty());
    REQUIRE_FALSE(capture.filter_hash.empty());

    auto missing_slot = request;
    missing_slot.selected_providers.erase(Astra::Media::AudioSlotId);
    Astra::Core::DiagnosticSink missing_slot_diagnostics;
    REQUIRE_FALSE(Astra::Media::ValidateMediaReleaseGate(missing_slot, missing_slot_diagnostics));
    REQUIRE(missing_slot_diagnostics.HasBlocking());

    auto slot_mismatch = request;
    slot_mismatch.selected_providers[Astra::Media::Renderer2DSlotId] = providers[2].provider_id;
    Astra::Core::DiagnosticSink mismatch_diagnostics;
    REQUIRE_FALSE(Astra::Media::ValidateMediaReleaseGate(slot_mismatch, mismatch_diagnostics));
    REQUIRE(mismatch_diagnostics.HasBlocking());

    auto not_packaged = request;
    not_packaged.providers[0].packaged_eligible = false;
    Astra::Core::DiagnosticSink packaged_diagnostics;
    REQUIRE_FALSE(Astra::Media::ValidateMediaReleaseGate(not_packaged, packaged_diagnostics));
    REQUIRE(packaged_diagnostics.HasBlocking());

    auto missing_prefix = request;
    missing_prefix.providers[1].diagnostics_code_prefix.clear();
    Astra::Core::DiagnosticSink prefix_diagnostics;
    REQUIRE_FALSE(Astra::Media::ValidateMediaReleaseGate(missing_prefix, prefix_diagnostics));
    REQUIRE(prefix_diagnostics.HasBlocking());
}

TEST_CASE("Media Phase 7 providers execute production DTO paths") {
    Astra::Core::DiagnosticSink diagnostics;
    auto providers = Astra::Media::ProductionMediaProviders();
    REQUIRE(providers.size() == 8);

    Astra::Media::MediaReleaseGateRequest request;
    request.providers = providers;
    for (const auto& provider : providers) {
        request.selected_providers[provider.slot_id] = provider.provider_id;
    }
    auto release = Astra::Media::ValidateMediaReleaseGate(request, diagnostics);
    REQUIRE(release);
    REQUIRE(release.Value().selected_providers.size() == 8);
    REQUIRE(release.Value().provider_hash_inputs.size() == 8);

    const auto fixture_png = ReadFixtureBytes(std::filesystem::path(ASTRA_SOURCE_ROOT) /
                                              "Samples/NativeVN/Content/UI/TextBox.png");
    REQUIRE_FALSE(fixture_png.empty());
    auto cpu_texture = Astra::Media::DecodeImageCpuBufferBytes(fixture_png, diagnostics);
    REQUIRE(cpu_texture);
    REQUIRE(cpu_texture.Value().row_stride == cpu_texture.Value().width * 4);

    const auto audio_payload = ReadFixtureBytes(std::filesystem::path(ASTRA_SOURCE_ROOT) /
                                                "Samples/NativeVN/Content/Music/opening_theme.ogg");
    REQUIRE_FALSE(audio_payload.empty());
    auto audio_decode = Astra::Media::DecodeAudioBytes(audio_payload, diagnostics);
    REQUIRE(audio_decode);
    REQUIRE(audio_decode.Value().pcm_frame_count > 0);

    const std::vector<Astra::Core::u8> fake_video(12);
    Astra::Core::DiagnosticSink video_diagnostics;
    REQUIRE_FALSE(Astra::Media::InspectVideoBytes(fake_video, video_diagnostics));
    REQUIRE(video_diagnostics.HasBlocking());

    auto renderer = Astra::Media::CreateHeadlessRenderer2DProvider();
    REQUIRE(renderer->BeginFrame({7, 1280, 720, "srgb"}, diagnostics));
    auto texture = renderer->ImportTexture(cpu_texture.Value(), diagnostics);
    REQUIRE(texture);
    REQUIRE_FALSE(texture.Value().Empty());

    std::vector<Astra::Media::PresentationCommand> commands;
    Astra::Media::PresentationCommand draw;
    draw.kind = Astra::Media::PresentationCommandKind::Sprite;
    draw.command_id = "draw.phase7.texture";
    draw.frame_index = 7;
    draw.layer = "ui";
    draw.asset = Astra::Asset::ParseAssetUri("native:/UI/TextBox").Value();
    commands.push_back(std::move(draw));
    auto graph = Astra::Media::ExtractRenderGraph(commands, nullptr, diagnostics);
    REQUIRE(renderer->Execute(graph, diagnostics));
    auto capture = renderer->Capture(diagnostics);
    REQUIRE(capture);
    REQUIRE_FALSE(capture.Value().render_hash.empty());
    REQUIRE(capture.Value().commands["imported_texture_count"] == 1);

    auto production_renderer =
        Astra::Media::CreateProductionRenderer2DProvider({1, "headless", 64, 64});
    Astra::Core::DiagnosticSink production_renderer_diagnostics;
    auto production_begin =
        production_renderer->BeginFrame({8, 64, 64, "srgb"}, production_renderer_diagnostics);
    if (Astra::Media::ProbeMediaBackendCapabilities().renderer2d_ready) {
        REQUIRE(production_begin);
    } else {
        REQUIRE_FALSE(production_begin);
    }

    auto text_provider = Astra::Media::CreateFoundationTextLayoutProvider();
    auto glyph = text_provider->Shape(
        {"text.phase7", "Phase 7 text 確認", "ja-JP", "text", 10, {{"size", 28}}}, diagnostics);
    REQUIRE(glyph);
    REQUIRE(glyph.Value().glyph_count > 0);
    REQUIRE(text_provider->PrepareAtlas(glyph.Value(), diagnostics));
    auto text_capture = text_provider->Capture(diagnostics);
    REQUIRE(text_capture);
    REQUIRE_FALSE(text_capture.Value().glyph_hash.empty());

    auto production_text_provider = Astra::Media::CreateProductionTextLayoutProvider();
    REQUIRE(production_text_provider->Describe().provider_id == "astra.text_layout.skia_ui");

    auto audio_provider = Astra::Media::CreateFoundationAudioProvider(true);
    auto music = Astra::Asset::ParseAssetUri("native:/Music/opening_theme").Value();
    REQUIRE(audio_provider->Submit({{"audio.phase7.music", "play", music, "music", 0.8, true}},
                                   diagnostics));
    auto audio_capture = audio_provider->Capture(diagnostics);
    REQUIRE(audio_capture);
    REQUIRE(audio_capture.Value().silent_backend);
    REQUIRE_FALSE(audio_capture.Value().state_hash.empty());

    Astra::Media::FilterProfile filter;
    filter.id = Astra::Asset::ParseAssetUri("native:/Filters/soft_vn").Value();
    filter.passes.push_back({"pass",
                             "astra.filter.color_grade",
                             Astra::Media::FilterTarget::Final,
                             {{"preset", "warm"}}});
    auto filter_execution = Astra::Media::ExecuteFilterGraphHeadless(filter, graph);
    REQUIRE(filter_execution.execution_mode == "headless_hash_fallback");
    REQUIRE_FALSE(filter_execution.output_hash.empty());

    const auto timeline_json = nlohmann::json{
        {"schema", "astra.media.timeline.v1"},
        {"id", "native:/Timelines/Opening"},
        {"duration_seconds", 3.0},
        {"tracks",
         nlohmann::json::array(
             {{{"id", "camera.main"},
               {"type", "camera"},
               {"keys",
                nlohmann::json::array({{{"t", 0.0}, {"value", {{"x", 0}, {"y", 0}, {"zoom", 1.0}}}},
                                       {{"t", 1.0},
                                        {"value", {{"x", 24}, {"y", 0}, {"zoom", 1.05}}},
                                        {"easing", "ease_out"}}})}},
              {{"id", "audio.bgm"},
               {"type", "audio"},
               {"audio_events", nlohmann::json::array({{{"command_id", "timeline.bgm"},
                                                        {"asset", "native:/Music/opening_theme"},
                                                        {"bus", "music"},
                                                        {"loop", true}}})}}})},
    };
    auto timeline = Astra::Media::TimelineFromJson(timeline_json, diagnostics);
    REQUIRE(timeline);
    auto state = Astra::Media::EvaluateTimeline(timeline.Value(), 1500000000ull);
    REQUIRE(state.camera["zoom"] == 1.05);
    REQUIRE_FALSE(state.pending_events.empty());
    REQUIRE(Astra::Media::ToJson(state)["schema"] == "astra.media.timeline_state.v1");
}

TEST_CASE("Media production driver diff and CPU FilterGraph hardening") {
    Astra::Core::DiagnosticSink diagnostics;
    auto filter_id = Astra::Asset::ParseAssetUri("native:/Filters/cpu");
    auto asset = Astra::Asset::ParseAssetUri("native:/Images/pixel");
    REQUIRE(filter_id);
    REQUIRE(asset);

    Astra::Media::FilterProfile profile;
    profile.id = filter_id.Value();
    profile.passes = {
        {"gray", "grayscale", Astra::Media::FilterTarget::Final, {}},
        {"opacity", "opacity", Astra::Media::FilterTarget::Final, {{"alpha", 0.5}}},
        {"tint", "tint", Astra::Media::FilterTarget::Final, {{"r", 1.0}, {"g", 0.5}, {"b", 0.5}}},
        {"blur", "blur3x3", Astra::Media::FilterTarget::Final, {}},
    };

    Astra::Media::RenderGraph graph;
    graph.frame_index = 1;
    graph.draws.push_back({"draw", asset.Value(), "final", 0, {}, {}});
    graph.text_requests.push_back({"text", "abc", "en", "text", 0, {}});
    graph.audio_commands.push_back({"audio", "play", asset.Value(), "sfx", 1.0, false});

    Astra::Media::DecodedCpuBuffer buffer;
    buffer.width = 3;
    buffer.height = 3;
    buffer.row_stride = 12;
    buffer.pixels.assign(3 * 3 * 4, 128);
    auto execution = Astra::Media::ExecuteFilterGraphCpu(profile, graph, &buffer, diagnostics);
    REQUIRE_FALSE(diagnostics.HasBlocking());
    REQUIRE(execution.execution_mode == "cpu_rgba");
    REQUIRE_FALSE(execution.output_hash.empty());

    auto headless = Astra::Media::CreateHeadlessRenderer2DProvider();
    REQUIRE(headless->BeginFrame({1, 64, 64, "srgb"}, diagnostics));
    REQUIRE(headless->Execute(graph, diagnostics));
    auto reference = headless->Capture(diagnostics);
    REQUIRE(reference);

    auto production = Astra::Media::CreateProductionRenderer2DProvider();
    auto capabilities = Astra::Media::ProbeMediaBackendCapabilities();
    if (production->BeginFrame({1, 64, 64, "srgb"}, diagnostics)) {
        REQUIRE(production->Execute(graph, diagnostics));
        auto candidate = production->Capture(diagnostics);
        REQUIRE(candidate);
        auto diff = Astra::Media::CompareDriverCaptures(reference.Value(), candidate.Value(),
                                                        capabilities, true);
        REQUIRE(Astra::Media::ToJson(diff)["schema"] == "astra.media.driver_diff.v1");
    } else {
        auto diff = Astra::Media::CompareDriverCaptures(reference.Value(), {}, capabilities, true);
        REQUIRE_FALSE(diff.passed);
    }
}
