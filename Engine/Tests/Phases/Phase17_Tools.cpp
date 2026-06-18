TEST_CASE("Tools reports validate inspect package and hash foundation artifacts") {
    Astra::Tools::CommandOptions options;
    options.strict = true;

    auto validation = Astra::Tools::Validate(
        std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/PackageSmoke", options);
    REQUIRE(validation.Passed());
    REQUIRE(Astra::Tools::ToJson(validation)["command"] == "astra validate");
    REQUIRE(validation.artifacts["foundation_core_gate"]["passed"] == true);
    REQUIRE(validation.artifacts["engine_binaries"].is_array());
    REQUIRE(validation.artifacts["phase3_media_backend_capabilities"]["schema"] ==
            "astra.media.backend_capabilities.v1");
    REQUIRE(validation.artifacts["phase3_media_release_gate"]["schema"] ==
            Astra::Media::MediaReleaseGateSchema);
    REQUIRE(validation.artifacts["phase3_media_release_gate"]["selected_providers"].size() == 3);

    auto native_validation = Astra::Tools::Validate(
        std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/NativeVN", options);
    REQUIRE(native_validation.Passed());
    REQUIRE(native_validation.artifacts["asset_registry"]["entries"].size() >= 7);
    REQUIRE(native_validation.artifacts["asset_dependency_graph"]["hard_edges"].contains(
        "native:/Scripts/opening"));
    REQUIRE(native_validation.artifacts["phase3_media_release_gate"]["passed"] == true);
    REQUIRE(native_validation.artifacts["phase8_script_vn"]["status"] == "passed");
    REQUIRE(native_validation.artifacts["phase4_script_vn"]["deprecated_alias_for"] ==
            "phase8_script_vn");
    REQUIRE(
        native_validation.artifacts["phase8_script_vn"]["parity"]["presentation_hashes_match"] ==
        true);
    REQUIRE_FALSE(native_validation.artifacts["phase8_script_vn"]["command_manifest"].empty());

    auto native_cook =
        Astra::Tools::Cook(std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/NativeVN", options);
    REQUIRE(native_cook.Passed());
    REQUIRE(std::filesystem::exists(native_cook.artifacts["ddc_manifest"].get<std::string>()));
    REQUIRE(native_cook.artifacts["cook_manifest"]["ddc_entries"].size() >= 7);
    REQUIRE(native_cook.artifacts["ddc_status"]["rebuilt"].get<Astra::Core::u64>() +
                native_cook.artifacts["ddc_status"]["reused"].get<Astra::Core::u64>() >=
            7);

    const auto ddc_root =
        std::filesystem::path(native_cook.artifacts["ddc_root"].get<std::string>());
    const auto first_cache_artifact =
        ddc_root / native_cook.artifacts["cook_manifest"]["ddc_entries"][0]["artifacts"][0]["path"]
                       .get<std::string>();
    REQUIRE(std::filesystem::exists(first_cache_artifact));
    {
        std::ofstream file(first_cache_artifact, std::ios::binary);
        file << "corrupted cache payload";
    }
    auto recovered_cook =
        Astra::Tools::Cook(std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/NativeVN", options);
    REQUIRE(recovered_cook.Passed());
    REQUIRE(
        recovered_cook.artifacts["ddc_status"]["corruption_recovered"].get<Astra::Core::u64>() >=
        1);

    const auto media_sample = std::filesystem::temp_directory_path() / "astra_media_cook_sample";
    std::filesystem::remove_all(media_sample);
    std::filesystem::create_directories(media_sample / "Content/Backgrounds");
    {
        std::ofstream descriptor(media_sample / "astra.sample.yaml", std::ios::binary);
        descriptor << "schema: astra.sample.v1\n";
        descriptor << "id: Samples/MediaCookSmoke\n";
        descriptor << "phase: 3\n";
        descriptor << "foundation_only: true\n";
    }
    {
        const auto png = TestPng1x1Rgba();
        std::ofstream image(media_sample / "Content/Backgrounds/Room.png", std::ios::binary);
        image.write(reinterpret_cast<const char*>(png.data()),
                    static_cast<std::streamsize>(png.size()));
    }
    {
        std::ofstream sidecar(media_sample / "Content/Backgrounds/Room.asset.yaml",
                              std::ios::binary);
        sidecar << "id: native:/Backgrounds/Room\n";
        sidecar << "schema: astra.asset.sidecar.v1\n";
        sidecar << "type: image\n";
        sidecar << "source_path: Room.png\n";
        sidecar << "display_name: Room\n";
        sidecar << "origin: HumanAuthored\n";
        sidecar << "license:\n  owner: project\n  usage: internal\n";
        sidecar << "review:\n  status: accepted\n";
        sidecar << "cook:\n  preset: background\n";
        sidecar << "dependencies:\n  hard: []\n  soft: []\n";
    }
    auto media_cook = Astra::Tools::Cook(media_sample, options);
    REQUIRE(media_cook.Passed());
    REQUIRE(media_cook.artifacts["cook_manifest"]["artifacts"].size() == 1);
    REQUIRE(media_cook.artifacts["cook_manifest"]["artifacts"][0]["metadata"]["format"] ==
            "astra.texture.source");
    REQUIRE(media_cook.artifacts["cook_manifest"]["artifacts"][0]["metadata"]["media_inspect"]
                                ["status"] == "decoded");
    REQUIRE(media_cook.artifacts["cook_manifest"]["artifacts"][0]["metadata"]["media_inspect"]
                                ["decoded_by"] == "libpng");
    REQUIRE(media_cook.artifacts["cook_manifest"]["artifacts"][0]["metadata"]["media_inspect"]
                                ["width"] == 1);
    REQUIRE(
        media_cook.artifacts["cook_manifest"]["artifacts"][0]["metadata"]["provider_feature_hash"]
            .get<std::string>()
            .size() == 64);

    const auto import_sample = std::filesystem::temp_directory_path() / "astra_import_cli_sample";
    std::filesystem::remove_all(import_sample);
    std::filesystem::create_directories(import_sample / "Imports");
    {
        std::ofstream descriptor(import_sample / "astra.sample.yaml", std::ios::binary);
        descriptor << "schema: astra.sample.v1\n";
        descriptor << "id: Samples/ImportCliSmoke\n";
        descriptor << "phase: 6\n";
        descriptor << "foundation_only: true\n";
    }
    const auto import_source = import_sample / "Imports/ImportedRoom.png";
    {
        const auto png = TestPng1x1Rgba();
        std::ofstream image(import_source, std::ios::binary);
        image.write(reinterpret_cast<const char*>(png.data()),
                    static_cast<std::streamsize>(png.size()));
    }
    Astra::Tools::CommandOptions import_options = options;
    import_options.import_asset_id = "native:/Backgrounds/ImportedRoom";
    import_options.import_asset_type = "image";
    import_options.import_preset = "background";
    import_options.import_license_owner = "project";
    import_options.import_license_usage = "internal";
    auto imported = Astra::Tools::Import(import_sample, import_source, import_options);
    REQUIRE(imported.Passed());
    REQUIRE(std::filesystem::exists(imported.artifacts["sidecar_path"].get<std::string>()));
    auto imported_validation = Astra::Tools::Validate(import_sample, options);
    REQUIRE(imported_validation.Passed());
    REQUIRE(imported_validation.artifacts["asset_registry"]["entries"].size() == 1);
    auto imported_cook = Astra::Tools::Cook(import_sample, options);
    REQUIRE(imported_cook.Passed());
    REQUIRE(imported_cook.artifacts["cook_manifest"]["artifacts"].size() == 1);
    auto imported_package = Astra::Tools::Package(import_sample, options);
    REQUIRE(imported_package.Passed());
    REQUIRE(imported_package.artifacts["package_manifest"]["payloads"][0]["compression"] == "zstd");
    auto imported_inspect =
        Astra::Tools::Inspect(imported_package.artifacts["package"].get<std::string>(), options);
    REQUIRE(imported_inspect.Passed());
    REQUIRE(imported_inspect.artifacts["package_mount"]["assets"].size() == 1);

    auto package = Astra::Tools::Package(
        std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/PackageSmoke", options);
    REQUIRE(package.Passed());
    REQUIRE(package.artifacts.contains("package"));
    REQUIRE(std::filesystem::exists(package.artifacts["package"].get<std::string>()));

    auto inspected =
        Astra::Tools::Inspect(package.artifacts["package"].get<std::string>(), options);
    REQUIRE(inspected.Passed());
    REQUIRE(inspected.artifacts["document"]["schema"] == Astra::Asset::PackageManifestSchema);
    REQUIRE(inspected.artifacts["package_manifest"]["schema"] ==
            Astra::Asset::PackageManifestSchema);

    Astra::Tools::CommandOptions deterministic_options = options;
    deterministic_options.compare = true;
    auto native_package = Astra::Tools::Package(
        std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/NativeVN", deterministic_options);
    REQUIRE(native_package.Passed());
    REQUIRE(native_package.artifacts["release_report"]["schema"] == "astra.release.report.v1");
    REQUIRE(native_package.artifacts["release_report"]["driver_diff"]["schema"] ==
            "astra.media.driver_diff.v1");
    REQUIRE(native_package.artifacts["package_manifest"]["profile"] == "deterministic");
    REQUIRE(native_package.artifacts["package_manifest"]["cook_manifest"]["artifacts"].size() >= 7);
    REQUIRE(native_package.artifacts["package_manifest"]["cook_manifest"]["ddc_entries"].size() >=
            native_package.artifacts["package_manifest"]["cook_manifest"]["artifacts"].size());
    REQUIRE(native_package.artifacts["package_manifest"]["payloads"].size() >=
            native_package.artifacts["package_manifest"]["cook_manifest"]["artifacts"].size());
    REQUIRE_FALSE(
        native_package.artifacts["package_manifest"]["cook_manifest"]["artifacts"][0]["ddc_key"]
            .get<std::string>()
            .empty());
    REQUIRE_FALSE(
        native_package.artifacts["package_manifest"]["package_hash"].get<std::string>().empty());
    REQUIRE(native_package
                .artifacts["package_manifest"]["runtime_evidence"]["asset_registry"]["entries"]
                .size() >= 7);
    REQUIRE(std::filesystem::exists(native_package.artifacts["golden_replay"].get<std::string>()));

    auto native_inspected =
        Astra::Tools::Inspect(native_package.artifacts["package"].get<std::string>(), options);
    REQUIRE(native_inspected.Passed());
    REQUIRE(native_inspected.artifacts["package_manifest"]["cook_manifest"]["ddc_entries"].size() >=
            7);
    REQUIRE(native_inspected.artifacts["package_mount"]["assets"].size() >= 7);
    REQUIRE(native_inspected.artifacts["payload_smoke"]["asset_id"].get<std::string>().starts_with(
        "native:/"));
    auto release_gate = Astra::Tools::ReleaseGate(
        std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/NativeVN", deterministic_options);
    REQUIRE(release_gate.Passed());
    REQUIRE(release_gate.artifacts["release_report"]["passed"] == true);
    REQUIRE(release_gate.artifacts["release_report"]["trace_events"].size() >= 2);
    REQUIRE(release_gate.artifacts["release_report"]["crash_bundle"]["schema"] ==
            "astra.crash.bundle.v1");

    Astra::Core::DiagnosticSink package_reader_diagnostics;
    Astra::Asset::PackageReader reader;
    auto opening_asset = Astra::Asset::ParseAssetUri("native:/Scripts/opening");
    REQUIRE(opening_asset);
    auto opening_payload =
        reader.ReadPayloadText(native_package.artifacts["package"].get<std::string>(),
                               opening_asset.Value(), package_reader_diagnostics);
    REQUIRE(opening_payload);
    REQUIRE(opening_payload.Value().find("story prologue") != std::string::npos);
    REQUIRE(opening_payload.Value().find("#@id scene_station") != std::string::npos);
    REQUIRE(opening_payload.Value().find("scene route_systems") != std::string::npos);
    auto opening_chunks =
        reader.ReadPayloadChunks(native_package.artifacts["package"].get<std::string>(),
                                 opening_asset.Value(), 32, package_reader_diagnostics);
    REQUIRE(opening_chunks);
    REQUIRE(opening_chunks.Value().size() > 1);
    REQUIRE(opening_chunks.Value().back().final_chunk);
    auto room_asset = Astra::Asset::ParseAssetUri("native:/Backgrounds/Room");
    REQUIRE(room_asset);
    auto room_payload =
        reader.ReadPayloadBytes(native_package.artifacts["package"].get<std::string>(),
                                room_asset.Value(), package_reader_diagnostics);
    REQUIRE(room_payload);
    auto room_rgba =
        Astra::Media::DecodeImageRgbaBytes(room_payload.Value(), package_reader_diagnostics);
    REQUIRE(room_rgba);
    REQUIRE(room_rgba.Value().pixels.size() ==
            static_cast<std::size_t>(room_rgba.Value().width) * room_rgba.Value().height * 4);
    auto music_asset = Astra::Asset::ParseAssetUri("native:/Music/opening_theme");
    REQUIRE(music_asset);
    auto music_payload =
        reader.ReadPayloadBytes(native_package.artifacts["package"].get<std::string>(),
                                music_asset.Value(), package_reader_diagnostics);
    REQUIRE(music_payload);
    auto music_decode =
        Astra::Media::DecodeAudioBytes(music_payload.Value(), package_reader_diagnostics);
    REQUIRE(music_decode);
    REQUIRE(music_decode.Value().pcm_frame_count > 0);
    auto package_mount = reader.MountPackage(native_package.artifacts["package"].get<std::string>(),
                                             package_reader_diagnostics);
    REQUIRE(package_mount);
    REQUIRE(package_mount.Value().assets.size() >= 7);

    const auto tampered_package =
        std::filesystem::temp_directory_path() / "astra_nativevn_tampered.astrapkg";
    {
        std::ofstream file(tampered_package, std::ios::binary);
        file << "not an astra package";
    }
    Astra::Core::DiagnosticSink package_diagnostics;
    auto tampered = reader.ReadManifest(tampered_package, package_diagnostics);
    REQUIRE_FALSE(tampered);
    REQUIRE(package_diagnostics.HasBlocking());

    const auto tampered_payload_package =
        std::filesystem::temp_directory_path() / "astra_nativevn_payload_tampered.astrapkg";
    std::filesystem::copy_file(native_package.artifacts["package"].get<std::string>(),
                               tampered_payload_package,
                               std::filesystem::copy_options::overwrite_existing);
    auto tampered_asset = Astra::Asset::ParseAssetUri(
        native_package.artifacts["package_manifest"]["payloads"][0]["asset_id"].get<std::string>());
    REQUIRE(tampered_asset);
    const auto tampered_offset =
        native_package.artifacts["package_manifest"]["payloads"][0]["offset"]
            .get<Astra::Core::u64>();
    {
        std::fstream file(tampered_payload_package,
                          std::ios::binary | std::ios::in | std::ios::out);
        file.seekg(static_cast<std::streamoff>(tampered_offset), std::ios::beg);
        char byte = 0;
        file.read(&byte, 1);
        file.seekp(static_cast<std::streamoff>(tampered_offset), std::ios::beg);
        byte = static_cast<char>(byte ^ 0x7f);
        file.write(&byte, 1);
    }
    Astra::Core::DiagnosticSink payload_diagnostics;
    auto tampered_payload = reader.ReadPayloadBytes(tampered_payload_package,
                                                    tampered_asset.Value(), payload_diagnostics);
    REQUIRE_FALSE(tampered_payload);
    REQUIRE(payload_diagnostics.HasBlocking());

    auto replay = Astra::Tools::Replay(native_package.artifacts["golden_replay"].get<std::string>(),
                                       deterministic_options);
    REQUIRE(replay.Passed());
    REQUIRE(replay.artifacts["comparison"]["passed"] == true);

    const auto descriptor = std::filesystem::path(ASTRA_PHASE1_PLUGIN_DESCRIPTOR);
    auto plugin = Astra::Tools::Validate(descriptor, options);
    REQUIRE(plugin.Passed());
    REQUIRE(plugin.artifacts["plugin"]["modules"][0].contains("sha256"));

    Astra::Tools::CommandOptions run_options;
    run_options.headless_smoke = true;
    auto run = Astra::Tools::Run(std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/PackageSmoke",
                                 run_options);
    REQUIRE(run.Passed());
    REQUIRE(run.artifacts["headless_smoke"]["phase3"]["media_release_gate"]["selected_providers"]
                .size() == 3);
    REQUIRE(run.artifacts["headless_smoke"]["phase3"]["media_backend_capabilities"]["schema"] ==
            "astra.media.backend_capabilities.v1");
    REQUIRE(run.artifacts["headless_smoke"]["phase3"]["image_decode_smoke"]["decoded_by"] ==
            "libpng");
    REQUIRE_FALSE(run.artifacts["headless_smoke"]["phase3"]["headless_capture"]["render_hash"]
                      .get<std::string>()
                      .empty());

    auto native_run = Astra::Tools::Run(
        std::filesystem::path(ASTRA_SOURCE_ROOT) / "Samples/NativeVN", run_options);
    REQUIRE(native_run.Passed());
    REQUIRE(native_run.artifacts["headless_smoke"]["phase8_script_vn"]["status"] == "passed");
    REQUIRE(native_run.artifacts["headless_smoke"]["phase4_script_vn"]["deprecated_alias_for"] ==
            "phase8_script_vn");

    auto package_run =
        Astra::Tools::Run(native_package.artifacts["package"].get<std::string>(), run_options);
    REQUIRE(package_run.Passed());
    REQUIRE(package_run.artifacts["headless_smoke"]["package_manifest"]["schema"] ==
            Astra::Asset::PackageManifestSchema);
    REQUIRE(package_run.artifacts["headless_smoke"]["package_payload_smoke"]["read_mode"] ==
            "random_access");
    REQUIRE(package_run.artifacts["headless_smoke"]["package_payload_smoke"]["stream_chunks"]
                .get<std::size_t>() > 1);
}
