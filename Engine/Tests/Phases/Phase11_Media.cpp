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
    profile.passes.push_back({"bg_blur", "astra.filter.gaussian_blur", Astra::Media::FilterTarget::Background, {{"radius", 2}}});
    profile.passes.push_back({"character_line", "astra.filter.line_enhance", Astra::Media::FilterTarget::Character, {{"strength", 0.4}}});
    profile.passes.push_back({"final_grade", "astra.filter.color_grade", Astra::Media::FilterTarget::Final, {{"preset", "warm"}}});
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
    const auto fixture_png = ReadFixtureBytes(std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/NativeVN/Content/Backgrounds/Room.png");
    REQUIRE_FALSE(fixture_png.empty());
    auto rgba = Astra::Media::DecodeImageRgbaBytes(fixture_png, diagnostics);
    REQUIRE(rgba);
    REQUIRE(rgba.Value().width > 1);
    REQUIRE(rgba.Value().height > 1);
    REQUIRE(rgba.Value().pixels.size() == static_cast<std::size_t>(rgba.Value().width) * rgba.Value().height * 4);
    REQUIRE(rgba.Value().decoded_by == "libpng");
    const auto font_payload = ReadFixtureBytes(std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/TsuiNoSora/Content/Fonts/sourcehanserif-medium.otf");
    REQUIRE_FALSE(font_payload.empty());
    auto rasterized_text = Astra::Media::RasterizeTextRgbaBytes(font_payload, "Aya route text 確認", 28, diagnostics);
    REQUIRE(rasterized_text);
    REQUIRE(rasterized_text.Value().glyph_count > 0);
    REQUIRE(rasterized_text.Value().width > 0);
    REQUIRE(rasterized_text.Value().height > 0);
    REQUIRE(rasterized_text.Value().pixels.size() == static_cast<std::size_t>(rasterized_text.Value().width) * rasterized_text.Value().height * 4);
    REQUIRE(rasterized_text.Value().rasterized_by == "freetype");
    const auto audio_payload = ReadFixtureBytes(std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/NativeVN/Content/Music/opening_theme.ogg");
    REQUIRE_FALSE(audio_payload.empty());
    auto audio_decode = Astra::Media::DecodeAudioBytes(audio_payload, diagnostics);
    REQUIRE(audio_decode);
    REQUIRE(audio_decode.Value().format == "ogg");
    REQUIRE(audio_decode.Value().channels > 0);
    REQUIRE(audio_decode.Value().sample_rate > 0);
    REQUIRE(audio_decode.Value().pcm_frame_count > 0);
    REQUIRE((audio_decode.Value().decoded_by.starts_with("ffmpeg/") || audio_decode.Value().decoded_by == "libvorbisfile" || audio_decode.Value().decoded_by == "miniaudio"));
    const auto artemis_bgm_payload = ReadFixtureBytes(std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/TsuiNoSora/Content/Music/bgm113.ogg");
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
    profile.passes.push_back({"bg_soften", "astra.filter.foundation_hash", Astra::Media::FilterTarget::Background, {{"strength", 0.25}}});
    profile.passes.push_back({"final_grade", "astra.filter.foundation_hash", Astra::Media::FilterTarget::Final, {{"grade", "warm"}}});

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



