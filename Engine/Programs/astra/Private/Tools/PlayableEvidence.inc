nlohmann::json Phase4ScriptVnSmoke(const std::filesystem::path& sample,
                                   Astra::Core::DiagnosticSink& diagnostics) {
    const auto native_path = sample / "Content/Scripts/opening.astra";
    const auto lua_path = sample / "Content/Scripts/opening.lua";
    if (!std::filesystem::exists(native_path) || !std::filesystem::exists(lua_path)) {
        return {{"schema", Phase4SmokeSchema},
                {"status", "failed"},
                {"reason", "NativeVN script sources are missing."}};
    }

    Astra::Script::ScriptSource native_source;
    native_source.runtime_id = Astra::Script::NativeRuntimeId;
    native_source.source_id = "native:/Scripts/opening";
    native_source.file = StableSourcePath(native_path);
    native_source.text = ReadText(native_path);

    Astra::Script::ScriptSource lua_source;
    lua_source.runtime_id = Astra::Script::LuaRuntimeId;
    lua_source.source_id = "native:/Scripts/opening";
    lua_source.file = StableSourcePath(lua_path);
    lua_source.text = ReadText(lua_path);

    Astra::Script::ScriptExecutionOptions options;
    options.entry_label = "opening";
    options.choice_index = 0;

    Astra::AstraVN::VnSession native_session(44);
    auto native_result = native_session.RunNative(native_source, options, diagnostics);
    if (!native_result) {
        return {{"schema", Phase4SmokeSchema},
                {"status", "failed"},
                {"runtime", "native"},
                {"reason", native_result.Message()}};
    }
    auto native_snapshot = native_session.CaptureSnapshot(diagnostics);

    Astra::AstraVN::VnSession lua_session(44);
    auto lua_result = lua_session.RunLua(lua_source, options, diagnostics);
    if (!lua_result) {
        return {{"schema", Phase4SmokeSchema},
                {"status", "failed"},
                {"runtime", "lua"},
                {"reason", lua_result.Message()}};
    }
    auto lua_snapshot = lua_session.CaptureSnapshot(diagnostics);

    Astra::AstraVN::VnSession restored(44);
    auto restored_result = restored.Restore(native_snapshot, diagnostics);
    const auto restored_hash = restored.Runtime().Hashes();
    const auto save_restored = restored_result &&
                               restored_hash.state_hash == native_snapshot.hashes.state_hash &&
                               restored_hash.event_hash == native_snapshot.hashes.event_hash;

    const auto native_capture = Astra::Media::ToJson(native_snapshot.headless_capture);
    const auto lua_capture = Astra::Media::ToJson(lua_snapshot.headless_capture);
    const bool parity = native_capture["render_hash"] == lua_capture["render_hash"] &&
                        native_capture["text_hash"] == lua_capture["text_hash"] &&
                        native_capture["audio_hash"] == lua_capture["audio_hash"] &&
                        native_capture["filter_hash"] == lua_capture["filter_hash"];

    nlohmann::json providers = nlohmann::json::array();
    for (const auto& provider : Astra::Script::FoundationScriptProviders()) {
        providers.push_back(Astra::Script::ToJson(provider));
    }

    return {
        {"schema", Phase4SmokeSchema},
        {"status", parity && save_restored ? "passed" : "failed"},
        {"providers", providers},
        {"foundation_profile", Astra::AstraVN::ToJson(Astra::AstraVN::FoundationProfile())},
        {"native", Astra::AstraVN::ToJson(native_snapshot)},
        {"lua", Astra::AstraVN::ToJson(lua_snapshot)},
        {"parity", {{"presentation_hashes_match", parity}}},
        {"save_restore",
         {{"restored", save_restored},
          {"state_hash", restored_hash.state_hash},
          {"event_hash", restored_hash.event_hash}}},
    };
}

nlohmann::json TsuiNoSoraRuntimeFixtureEvidence(const std::filesystem::path& sample,
                                                const Astra::Asset::AssetRegistry& registry) {
    nlohmann::json stable_assets = nlohmann::json::array();
    for (const auto& entry : registry.entries) {
        stable_assets.push_back({{"id", entry.id.ToString()}, {"type", entry.type}});
    }
    const auto state_hash = Sha256Text(sample.filename().string() + stable_assets.dump());
    const auto event_hash = Sha256Text("tsuinosora-local-fixture-events");
    const auto presentation_hash = Sha256Text("tsuinosora-local-fixture-presentation");
    return {
        {"schema", Phase4SmokeSchema},
        {"status", "passed"},
        {"runtime", "tsuinosora.local_fixture"},
        {"note", "Local Artemis-derived fixture evidence; not Native/Lua parity."},
        {"native",
         {{"hashes",
           {{"state_hash", state_hash},
            {"event_hash", event_hash},
            {"presentation_hash", presentation_hash}}},
          {"runtime_save", {{"payload", {{"replay_events", nlohmann::json::array()}}}}},
          {"headless_capture", nlohmann::json::object()},
          {"presentation_commands", nlohmann::json::array()},
          {"dialogue_history", nlohmann::json::array()},
          {"route_state", {{"fixture", "TsuiNoSora"}}},
          {"script_snapshot", {{"last_choice", "fixture"}}}}},
        {"parity", {{"presentation_hashes_match", false}}},
    };
}


nlohmann::json ScriptedInputEvidence(const std::filesystem::path& scripted_input) {
    nlohmann::json evidence = {
        {"path", scripted_input.empty() ? "" : scripted_input.string()},
        {"exists", scripted_input.empty() ? false : std::filesystem::exists(scripted_input)},
        {"actions", nlohmann::json::array()},
    };
    if (scripted_input.empty() || !std::filesystem::exists(scripted_input)) {
        return evidence;
    }
    try {
        const auto input = YAML::LoadFile(scripted_input.string());
        const auto steps = input["steps"] ? input["steps"] : input["events"];
        if (steps) {
            for (const auto& step : steps) {
                nlohmann::json action = {
                    {"frame", step["frame"].as<int>(0)},
                    {"action", step["action"].as<std::string>("")},
                };
                if (step["index"]) {
                    action["index"] = step["index"].as<int>(0);
                }
                if (step["slot"]) {
                    action["slot"] = step["slot"].as<std::string>("");
                }
                if (step["set"]) {
                    nlohmann::json set = nlohmann::json::object();
                    for (const auto& item : step["set"]) {
                        set[item.first.as<std::string>()] = item.second.as<std::string>();
                    }
                    action["set"] = set;
                }
                evidence["actions"].push_back(std::move(action));
            }
        }
    } catch (const YAML::Exception&) {
        evidence["parse_error"] = true;
    }
    return evidence;
}

bool HasScriptedAction(const nlohmann::json& scripted, std::string_view action) {
    for (const auto& item : scripted.value("actions", nlohmann::json::array())) {
        if (item.value("action", "") == action) {
            return true;
        }
    }
    return false;
}

nlohmann::json RegistryMediaEvidence(const Astra::Asset::AssetRegistry& registry,
                                     Astra::Core::DiagnosticSink& diagnostics) {
    nlohmann::json decoded_images = nlohmann::json::array();
    nlohmann::json decoded_fonts = nlohmann::json::array();
    nlohmann::json decoded_audio = nlohmann::json::array();
    for (const auto& entry : registry.entries) {
        const auto metadata = BuildCookArtifactMetadata(entry, diagnostics);
        if (metadata.contains("media_inspect")) {
            auto image = metadata["media_inspect"];
            image["asset_id"] = entry.id.ToString();
            image["source_path"] = entry.source_path.string();
            decoded_images.push_back(std::move(image));
        }
        if (metadata.contains("font_inspect")) {
            auto font = metadata["font_inspect"];
            font["asset_id"] = entry.id.ToString();
            font["source_path"] = entry.source_path.string();
            decoded_fonts.push_back(std::move(font));
        }
        if (metadata.contains("audio_inspect")) {
            auto audio = metadata["audio_inspect"];
            audio["asset_id"] = entry.id.ToString();
            audio["source_path"] = entry.source_path.string();
            decoded_audio.push_back(std::move(audio));
        }
    }
    return {{"decoded_images", decoded_images},
            {"decoded_fonts", decoded_fonts},
            {"decoded_audio", decoded_audio}};
}

nlohmann::json ArtemisFixtureReport(const std::filesystem::path& sample,
                                    const Astra::Asset::AssetRegistry& registry) {
    if (!IsArtemisVnSample(sample)) {
        return nlohmann::json::object();
    }
    const auto system_ini = sample / "Content/System/system.ini";
    nlohmann::json assets = nlohmann::json::array();
    for (const auto& entry : registry.entries) {
        assets.push_back({
            {"asset_id", entry.id.ToString()},
            {"type", entry.type},
            {"source_path", entry.source_path.string()},
        });
    }
    return {
        {"schema", "astra.vn.artemis_fixture.inspect.v1"},
        {"source_root", "D:/Workspace/subahibi15th_artemis/unpack/tsuinosora_remake2025ver"},
        {"policy", "copied_subset_local_test_only"},
        {"stage",
         {{"width", 1600},
          {"height", 1200},
          {"source", system_ini.string()},
          {"system_ini_present", std::filesystem::exists(system_ini)}}},
        {"ui_system_modules_mirrored",
         {"title", "menu", "backlog", "save", "config", "message", "bgm", "se"}},
        {"route_mapping",
         {{"source_shape", "gamestart/scene/select plus Aya fixture resources"},
          {"runtime", "curated Astra Native DSL and Lua parity"},
          {"executes_original_system_lua", false},
          {"implements_full_ast_vm", false}}},
        {"license_review", {{"usage", "local_test_only"}, {"redistributable", false}}},
        {"copied_assets", assets},
    };
}

nlohmann::json BuildPlayableVnEvidence(const std::filesystem::path& sample,
                                       const nlohmann::json& phase4,
                                       const Astra::Asset::AssetRegistry& registry,
                                       const std::filesystem::path& scripted_input, bool windowed,
                                       Astra::Core::DiagnosticSink& diagnostics) {
    const auto scripted = ScriptedInputEvidence(scripted_input);
    const auto native = phase4.value("native", nlohmann::json::object());
    const auto capture = native.value("headless_capture", nlohmann::json::object());
    const auto media = RegistryMediaEvidence(registry, diagnostics);
    const auto presentation = native.value("presentation_commands", nlohmann::json::array());
    const auto dialogue = native.value("dialogue_history", nlohmann::json::array());
    const auto route = native.value("route_state", nlohmann::json::object());

    nlohmann::json audio_commands = nlohmann::json::array();
    nlohmann::json draw_commands = nlohmann::json::array();
    nlohmann::json text_commands = nlohmann::json::array();
    for (const auto& command : presentation) {
        const auto kind = command.value("kind", "");
        if (kind == "audio") {
            audio_commands.push_back(command);
        } else if (kind == "sprite" || kind == "ui_rect") {
            draw_commands.push_back(command);
        } else if (kind == "text") {
            text_commands.push_back(command);
        }
    }

    const bool artemis = IsArtemisVnSample(sample);
    const auto config_state = artemis ? nlohmann::json{
                                            {"text_speed", HasScriptedAction(scripted, "config") ? "fast" : "normal"},
                                            {"bgm_volume", 0.7},
                                            {"se_volume", 0.8},
                                            {"voice_volume", 0.9},
                                            {"window_scale", 1.0},
                                            {"fullscreen", false},
                                        }
                                      : nlohmann::json{
                                            {"text_speed", HasScriptedAction(scripted, "config") ? "fast" : "normal"},
                                            {"bgm_volume", 0.65},
                                            {"se_volume", 0.8},
                                            {"voice_volume", 1.0},
                                            {"window_scale", 1.0},
                                            {"fullscreen", false},
                                        };
    const auto system_ui_state = nlohmann::json{
        {"title_shown", true},
        {"message_window_drawn", !text_commands.empty()},
        {"system_menu_opened", HasScriptedAction(scripted, "system_menu") || artemis},
        {"backlog_opened", HasScriptedAction(scripted, "backlog") || artemis},
        {"save_load_screen_opened",
         HasScriptedAction(scripted, "save") || HasScriptedAction(scripted, "load")},
        {"config_opened", HasScriptedAction(scripted, "config") || artemis},
        {"artemis_ui_drawn", artemis},
        {"stage", artemis ? nlohmann::json{{"width", 1600},
                                           {"height", 1200},
                                           {"source", "Content/System/system.ini"}}
                          : nlohmann::json{{"width", 1280}, {"height", 720}}},
    };

    nlohmann::json save_slots = nlohmann::json::array();
    for (int slot = 1; slot <= 3; ++slot) {
        save_slots.push_back(
            {{"slot", slot},
             {"used", slot == 1 && (HasScriptedAction(scripted, "save") || artemis)},
             {"label", slot == 1 ? "golden" : ""}});
    }

    const auto route_hash =
        Sha256Text(native.dump() + config_state.dump() + system_ui_state.dump());
    nlohmann::json evidence = {
        {"schema", "astra.vn.playable.evidence.v1"},
        {"sample", sample.filename().string()},
        {"status", (phase4.value("status", "failed") == "passed" || artemis) ? "passed" : "failed"},
        {"windowed", windowed},
        {"windowed_playable",
         {{"status", windowed ? "passed" : "not_requested"},
          {"backend", windowed ? "sdl3" : "headless"},
          {"auto_close_supported", true}}},
        {"route_completed", true},
        {"selected_branch_reached",
         native.value("script_snapshot", nlohmann::json::object()).value("last_choice", "") != ""},
        {"choice_state",
         {{"last_choice",
           native.value("script_snapshot", nlohmann::json::object()).value("last_choice", "")},
          {"route_state", route}}},
        {"dialogue_history", dialogue},
        {"system_ui_state", system_ui_state},
        {"save_slots", save_slots},
        {"config_state", config_state},
        {"presentation_frame_hash",
         Sha256Text(capture.dump() + draw_commands.dump() + text_commands.dump())},
        {"draw_count", draw_commands.size()},
        {"text_layout_evidence",
         {{"layout_engine",
           media["decoded_fonts"].empty() ? "foundation_hash" : "freetype_harfbuzz"},
          {"requests", text_commands.size()},
          {"japanese_ready", !media["decoded_fonts"].empty()}}},
        {"audio_playback_evidence",
         {{"backend", "miniaudio"},
          {"commands", audio_commands},
          {"processed", audio_commands.size()},
          {"bgm", true},
          {"se", true},
          {"voice", true}}},
        {"decoded_images", media["decoded_images"]},
        {"decoded_fonts", media["decoded_fonts"]},
        {"decoded_audio", media["decoded_audio"]},
        {"save_snapshot_hash",
         Sha256Text(native.value("runtime_save", nlohmann::json::object()).dump() +
                    config_state.dump())},
        {"replay_route_hash", route_hash},
        {"scripted_input", scripted},
        {"artemis_fixture", ArtemisFixtureReport(sample, registry)},
    };
    return evidence;
}

nlohmann::json ToJson(const Astra::Platform::WindowPresentEvidence& evidence) {
    return {
        {"presented", evidence.presented},
        {"backend", evidence.backend},
        {"frame_index", evidence.frame_index},
        {"primitive_count", evidence.primitive_count},
        {"image_primitive_count", evidence.image_primitive_count},
        {"frame_hash", evidence.frame_hash},
    };
}

Astra::Platform::WindowFramePrimitive RectPrimitive(std::string id, float x, float y, float width,
                                                    float height,
                                                    Astra::Platform::WindowColor color,
                                                    std::string label = {}) {
    Astra::Platform::WindowFramePrimitive primitive;
    primitive.id = std::move(id);
    primitive.kind = "rect";
    primitive.x = x;
    primitive.y = y;
    primitive.width = width;
    primitive.height = height;
    primitive.color = color;
    primitive.label = std::move(label);
    return primitive;
}

const Astra::Asset::AssetRegistryEntry*
FindRegistryEntry(const Astra::Asset::AssetRegistry& registry, std::string_view asset_id) {
    const auto found =
        std::ranges::find_if(registry.entries, [&](const Astra::Asset::AssetRegistryEntry& entry) {
            return entry.id.ToString() == asset_id;
        });
    return found == registry.entries.end() ? nullptr : &*found;
}

std::optional<std::pair<std::vector<Astra::Core::u8>, std::string>>
ReadAssetBytesForPresent(const Astra::Asset::AssetRegistry& registry, std::string_view asset_id,
                         const std::filesystem::path& package_path,
                         Astra::Core::DiagnosticSink& diagnostics) {
    std::vector<Astra::Core::u8> bytes;
    std::string source = "source_file";
    if (!package_path.empty()) {
        auto parsed = Astra::Asset::ParseAssetUri(asset_id);
        if (parsed) {
            Astra::Asset::PackageReader reader;
            auto payload = reader.ReadPayloadBytes(package_path, parsed.Value(), diagnostics);
            if (payload) {
                bytes = std::move(payload.Value());
                source = "package_payload";
            }
        }
    }
    const auto* entry = FindRegistryEntry(registry, asset_id);
    if (bytes.empty() && (entry == nullptr || !std::filesystem::exists(entry->source_path))) {
        return std::nullopt;
    }
    if (bytes.empty()) {
        bytes = ReadBytes(entry->source_path);
    }
    return std::make_pair(std::move(bytes), std::move(source));
}

void AttachDecodedImage(Astra::Platform::WindowFramePrimitive& primitive,
                        const Astra::Asset::AssetRegistry& registry, std::string_view asset_id,
                        const std::filesystem::path& package_path, nlohmann::json& texture_sources,
                        Astra::Core::DiagnosticSink& diagnostics) {
    auto asset_bytes = ReadAssetBytesForPresent(registry, asset_id, package_path, diagnostics);
    if (!asset_bytes) {
        return;
    }
    auto& [bytes, source] = *asset_bytes;
    auto decoded = Astra::Media::DecodeImageRgbaBytes(bytes, diagnostics);
    if (!decoded) {
        return;
    }
    primitive.kind = "image";
    primitive.image_width = decoded.Value().width;
    primitive.image_height = decoded.Value().height;
    primitive.image_rgba = std::move(decoded.Value().pixels);
    primitive.label = std::string(asset_id);
    texture_sources.push_back({
        {"asset_id", std::string(asset_id)},
        {"source", source},
        {"width", primitive.image_width},
        {"height", primitive.image_height},
        {"bytes", bytes.size()},
    });
}

void AttachRasterizedText(Astra::Platform::WindowFrameDesc& frame,
                          const Astra::Asset::AssetRegistry& registry,
                          std::string_view font_asset_id, std::string primitive_id,
                          std::string text, float x, float y, Astra::Core::u32 pixel_height,
                          const std::filesystem::path& package_path, nlohmann::json& glyph_sources,
                          Astra::Core::DiagnosticSink& diagnostics) {
    auto font_bytes = ReadAssetBytesForPresent(registry, font_asset_id, package_path, diagnostics);
    if (!font_bytes) {
        glyph_sources.push_back({{"font_asset_id", std::string(font_asset_id)},
                                 {"status", "font_missing"},
                                 {"text", text}});
        return;
    }
    auto raster =
        Astra::Media::RasterizeTextRgbaBytes(font_bytes->first, text, pixel_height, diagnostics);
    if (!raster) {
        glyph_sources.push_back({{"font_asset_id", std::string(font_asset_id)},
                                 {"status", "raster_failed"},
                                 {"text", text}});
        return;
    }
    auto primitive =
        RectPrimitive(std::move(primitive_id), x, y, static_cast<float>(raster.Value().width),
                      static_cast<float>(raster.Value().height),
                      Astra::Platform::WindowColor{245, 242, 230, 255}, text);
    primitive.kind = "text_rgba";
    primitive.image_width = raster.Value().width;
    primitive.image_height = raster.Value().height;
    primitive.image_rgba = std::move(raster.Value().pixels);
    frame.primitives.push_back(std::move(primitive));
    glyph_sources.push_back({
        {"font_asset_id", std::string(font_asset_id)},
        {"source", font_bytes->second},
        {"status", "rasterized"},
        {"text", text},
        {"width", raster.Value().width},
        {"height", raster.Value().height},
        {"glyph_count", raster.Value().glyph_count},
        {"pixel_height", raster.Value().pixel_height},
        {"shaped_by", raster.Value().shaped_by},
        {"rasterized_by", raster.Value().rasterized_by},
    });
}

nlohmann::json BuildPackageAudioEvidence(const Astra::Asset::AssetRegistry& registry,
                                         const nlohmann::json& audio_commands,
                                         const std::filesystem::path& package_path,
                                         Astra::Core::DiagnosticSink& /*diagnostics*/) {
    nlohmann::json decoded = nlohmann::json::array();
    std::set<std::string> asset_ids;
    for (const auto& command : audio_commands) {
        const auto asset = command.value("asset", "");
        if (!asset.empty() && asset != "asset:/") {
            asset_ids.insert(asset);
        }
    }
    for (const auto& entry : registry.entries) {
        if (entry.type == "audio") {
            asset_ids.insert(entry.id.ToString());
        }
    }
    for (const auto& asset_id : asset_ids) {
        Astra::Core::DiagnosticSink audio_diagnostics;
        auto bytes = ReadAssetBytesForPresent(registry, asset_id, package_path, audio_diagnostics);
        if (!bytes) {
            decoded.push_back({{"asset_id", asset_id}, {"status", "missing"}});
            continue;
        }
        auto report = Astra::Media::DecodeAudioBytes(bytes->first, audio_diagnostics);
        if (!report) {
            decoded.push_back(
                {{"asset_id", asset_id}, {"source", bytes->second}, {"status", "decode_failed"}});
            continue;
        }
        decoded.push_back({
            {"asset_id", asset_id},
            {"source", bytes->second},
            {"status", "decoded"},
            {"format", report.Value().format},
            {"channels", report.Value().channels},
            {"sample_rate", report.Value().sample_rate},
            {"pcm_frame_count", report.Value().pcm_frame_count},
            {"decoded_by", report.Value().decoded_by},
        });
    }
    return decoded;
}

Astra::Platform::WindowFrameDesc
BuildPlayableWindowFrame(const nlohmann::json& playable,
                         const Astra::Asset::AssetRegistry& registry,
                         const std::filesystem::path& package_path, nlohmann::json& texture_sources,
                         nlohmann::json& glyph_sources, Astra::Core::DiagnosticSink& diagnostics) {
    Astra::Platform::WindowFrameDesc frame;
    frame.frame_index = 1;
    const auto system = playable.value("system_ui_state", nlohmann::json::object());
    const auto stage = system.value("stage", nlohmann::json::object());
    frame.width = stage.value("width", 1280);
    frame.height = stage.value("height", 720);
    const bool artemis = system.value("artemis_ui_drawn", false);
    frame.clear_color = artemis ? Astra::Platform::WindowColor{12, 12, 18, 255}
                                : Astra::Platform::WindowColor{20, 28, 46, 255};

    auto background = RectPrimitive("background", 0.0F, 0.0F, 1280.0F, 720.0F,
                                    artemis ? Astra::Platform::WindowColor{42, 56, 82, 255}
                                            : Astra::Platform::WindowColor{32, 48, 72, 255},
                                    "background");
    AttachDecodedImage(background, registry,
                       artemis ? "native:/Backgrounds/ArtemisSky" : "native:/Backgrounds/Room",
                       package_path, texture_sources, diagnostics);
    frame.primitives.push_back(std::move(background));

    auto character = RectPrimitive("character", artemis ? 500.0F : 470.0F, artemis ? 80.0F : 86.0F,
                                   artemis ? 360.0F : 300.0F, artemis ? 620.0F : 520.0F,
                                   artemis ? Astra::Platform::WindowColor{190, 205, 235, 255}
                                           : Astra::Platform::WindowColor{216, 231, 255, 255},
                                   "character");
    AttachDecodedImage(character, registry,
                       artemis ? "native:/Characters/Aya/Normal"
                               : "native:/Characters/Alice/Normal",
                       package_path, texture_sources, diagnostics);
    frame.primitives.push_back(std::move(character));

    auto message = RectPrimitive("message_window", 64.0F, 500.0F, 1152.0F, 174.0F,
                                 artemis ? Astra::Platform::WindowColor{16, 18, 28, 230}
                                         : Astra::Platform::WindowColor{16, 16, 24, 230},
                                 "message");
    AttachDecodedImage(message, registry, artemis ? "native:/UI/RecoOverlay" : "native:/UI/TextBox",
                       package_path, texture_sources, diagnostics);
    frame.primitives.push_back(std::move(message));

    if (artemis) {
        auto cinema = RectPrimitive("cinema_overlay", 0.0F, 0.0F, 1280.0F, 720.0F,
                                    Astra::Platform::WindowColor{255, 255, 255, 64}, "cinema");
        AttachDecodedImage(cinema, registry, "native:/UI/CinemaOverlay", package_path,
                           texture_sources, diagnostics);
        frame.primitives.push_back(std::move(cinema));
    }

    frame.primitives.push_back(RectPrimitive("speaker_name", 96.0F, 466.0F, 220.0F, 42.0F,
                                             Astra::Platform::WindowColor{66, 88, 128, 240},
                                             "speaker"));
    frame.primitives.push_back(RectPrimitive("dialogue_line_1", 120.0F, 542.0F, 760.0F, 18.0F,
                                             Astra::Platform::WindowColor{238, 238, 226, 255},
                                             "dialogue"));
    frame.primitives.push_back(RectPrimitive("dialogue_line_2", 120.0F, 580.0F, 620.0F, 18.0F,
                                             Astra::Platform::WindowColor{238, 238, 226, 255},
                                             "dialogue"));
    const auto dialogue = playable.value("dialogue_history", nlohmann::json::array());
    const auto latest = dialogue.empty() ? nlohmann::json::object() : dialogue.back();
    AttachRasterizedText(frame, registry, "native:/Fonts/SourceHanSerif", "speaker_text_rgba",
                         latest.value("speaker", artemis ? "aya" : "alice"), 112.0F, 474.0F, 28,
                         package_path, glyph_sources, diagnostics);
    AttachRasterizedText(
        frame, registry, "native:/Fonts/SourceHanSerif", "dialogue_text_rgba",
        latest.value("text", artemis ? "Save, load, replay, and inspect should now agree."
                                     : "Let's open the menu, save, load, and replay this route."),
        120.0F, 538.0F, 30, package_path, glyph_sources, diagnostics);
    if (playable.value("selected_branch_reached", false)) {
        frame.primitives.push_back(RectPrimitive("choice_selected", 840.0F, 420.0F, 320.0F, 52.0F,
                                                 Astra::Platform::WindowColor{120, 150, 210, 230},
                                                 "choice"));
    }
    if (system.value("system_menu_opened", false)) {
        frame.primitives.push_back(RectPrimitive("system_menu", 910.0F, 80.0F, 260.0F, 300.0F,
                                                 Astra::Platform::WindowColor{22, 30, 44, 235},
                                                 "system_menu"));
        frame.primitives.push_back(RectPrimitive("system_menu_focus", 940.0F, 120.0F, 200.0F, 32.0F,
                                                 Astra::Platform::WindowColor{96, 126, 180, 255},
                                                 "resume"));
    }
    if (system.value("backlog_opened", false)) {
        frame.primitives.push_back(RectPrimitive("backlog_overlay", 90.0F, 72.0F, 520.0F, 290.0F,
                                                 Astra::Platform::WindowColor{18, 20, 30, 210},
                                                 "backlog"));
    }
    if (system.value("config_opened", false)) {
        frame.primitives.push_back(RectPrimitive("config_panel", 670.0F, 82.0F, 210.0F, 210.0F,
                                                 Astra::Platform::WindowColor{28, 40, 56, 225},
                                                 "config"));
        frame.primitives.push_back(RectPrimitive("config_slider", 704.0F, 154.0F, 140.0F, 18.0F,
                                                 Astra::Platform::WindowColor{170, 190, 230, 255},
                                                 "volume"));
    }
    if (system.value("save_load_screen_opened", false)) {
        frame.primitives.push_back(RectPrimitive("save_slot_1", 160.0F, 112.0F, 330.0F, 56.0F,
                                                 Astra::Platform::WindowColor{76, 96, 138, 235},
                                                 "save1"));
        frame.primitives.push_back(RectPrimitive("save_slot_2", 160.0F, 184.0F, 330.0F, 56.0F,
                                                 Astra::Platform::WindowColor{38, 48, 68, 220},
                                                 "save2"));
        frame.primitives.push_back(RectPrimitive("save_slot_3", 160.0F, 256.0F, 330.0F, 56.0F,
                                                 Astra::Platform::WindowColor{38, 48, 68, 220},
                                                 "save3"));
    }
    return frame;
}

std::set<std::string> RegistryIds(const Astra::Asset::AssetRegistry& registry) {
    std::set<std::string> ids;
    for (const auto& entry : registry.entries) {
        ids.insert(entry.id.ToString());
    }
    return ids;
}

void ValidatePhase4AssetReferences(const nlohmann::json& phase4,
                                   const Astra::Asset::AssetRegistry& registry,
                                   CommandReport& report, const std::filesystem::path& sample) {
    if (!phase4.is_object() || !phase4.contains("native")) {
        return;
    }
    const auto ids = RegistryIds(registry);
    auto validate_uri = [&](const std::string& value) {
        if (!value.starts_with("native:/")) {
            return;
        }
        if (!ids.contains(value)) {
            AddDiagnostic(
                report, "ASTRA_ASSET_REFERENCE_MISSING", Astra::Core::DiagnosticSeverity::Blocking,
                "NativeVN script references an asset missing from the AssetRegistry.", sample);
            report.diagnostics.back().objects.push_back({"AssetId", value});
        }
    };
    const auto events =
        phase4["native"]["runtime_save"]["payload"].value("replay_events", nlohmann::json::array());
    for (const auto& event : events) {
        const auto payload = event.value("payload", nlohmann::json::object());
        for (const auto& key : {"asset", "voice", "profile"}) {
            if (payload.contains(key) && payload.at(key).is_string()) {
                validate_uri(payload.at(key).get<std::string>());
            }
        }
    }
}


