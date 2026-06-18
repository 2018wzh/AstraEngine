TEST_CASE("Headless platform provides filesystem timer thread and crash services") {
    Astra::Core::DiagnosticSink create_diagnostics;
    auto created = Astra::Platform::CreatePlatform({}, create_diagnostics);
    REQUIRE(created);
    auto platform = std::move(created.Value());
    REQUIRE(platform.Kind() == Astra::Platform::BackendKind::Headless);
    bool ran = false;
    platform.Thread().DispatchTagged("phase1", [&] { ran = true; });
    REQUIRE(platform.Thread().PendingTags()[0] == "phase1");
    platform.Thread().Drain();
    REQUIRE(ran);
    REQUIRE(platform.Thread().CompletedTags()[0] == "phase1");
    REQUIRE(platform.Thread().PendingTags().empty());
    REQUIRE(platform.Timer().MonotonicNanoseconds() > 0);
    REQUIRE(platform.FileSystem().Mount("project", std::filesystem::temp_directory_path(), false));
    REQUIRE(platform.FileSystem().Resolve("project", "demo.txt").filename() == "demo.txt");
    bool watch_triggered = false;
    const auto watch_root = std::filesystem::temp_directory_path() / "astra_phase1_watch";
    std::filesystem::create_directories(watch_root);
    platform.FileSystem().Watch(watch_root, [&](const std::filesystem::path&) { watch_triggered = true; });
    REQUIRE(platform.FileSystem().WriteText(watch_root / "changed.txt", "changed"));
    platform.FileSystem().PollWatches();
    REQUIRE(watch_triggered);
    auto missing_symbol = platform.DynamicLibrary().Symbol({}, "missing");
    REQUIRE_FALSE(missing_symbol);
    REQUIRE_FALSE(platform.Input().Snapshot().quit_requested);
    platform.Clipboard().SetText("phase1");
    REQUIRE(platform.Clipboard().GetText() == "phase1");
    platform.Cursor().SetVisible(false);
    REQUIRE_FALSE(platform.Cursor().IsVisible());
    REQUIRE(platform.Display().Displays().size() == 1);
    Astra::Core::DiagnosticSink window_diagnostics;
    REQUIRE(platform.Window().Create({"Headless Present Test", 320, 180}, window_diagnostics));
    Astra::Platform::WindowFrameDesc frame;
    frame.width = 320;
    frame.height = 180;
    frame.clear_color = {5, 8, 10, 255};
    Astra::Platform::WindowFramePrimitive primitive;
    primitive.id = "dialogue";
    primitive.x = 8.0f;
    primitive.y = 12.0f;
    primitive.width = 120.0f;
    primitive.height = 48.0f;
    primitive.color = {102, 153, 204, 255};
    primitive.label = "dialogue";
    frame.primitives.push_back(primitive);
    const auto presented = platform.Window().PresentFrame(frame, window_diagnostics);
    REQUIRE(presented);
    REQUIRE(presented.Value().presented);
    REQUIRE(presented.Value().backend == "headless");
    REQUIRE(presented.Value().primitive_count == 1);
    REQUIRE(presented.Value().image_primitive_count == 0);
    REQUIRE_FALSE(presented.Value().frame_hash.empty());
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = "ASTRA_TEST";
    diagnostic.category = "test";
    diagnostic.severity = Astra::Core::DiagnosticSeverity::Warning;
    diagnostic.message = "test";
    diagnostics.Emit(std::move(diagnostic));
    Astra::Platform::CrashCaptureContext crash_context;
    crash_context.build_info = "build";
    crash_context.frame_index = 42;
    crash_context.package_or_project_hash = "hash";
    crash_context.recent_logs = {"last log"};
    auto crash = platform.Crash().Capture(std::move(crash_context), diagnostics);
    REQUIRE(crash.diagnostics.size() == 1);
    REQUIRE(crash.frame_index == 42);
    REQUIRE_FALSE(crash.thread_id.empty());
    REQUIRE(crash.recent_logs[0] == "last log");
}



TEST_CASE("Platform target table describes host distribution data and backend capabilities") {
    const auto host = Astra::Platform::CurrentHostTargetPlatform();
    REQUIRE_FALSE(host.id.empty());
#if defined(_WIN32)
    REQUIRE(host.id == "win64");
#elif defined(__APPLE__)
    REQUIRE((host.id == "macos-x64" || host.id == "macos-arm64"));
#else
    REQUIRE(host.id == "linux-x64");
#endif

    auto win64 = Astra::Platform::FindTargetPlatform("win64");
    REQUIRE(win64);
    REQUIRE(win64.Value().launcher_name == "astra.exe");
    REQUIRE(win64.Value().dynamic_library_extension == ".dll");
    REQUIRE(win64.Value().plugin_bin_dir == "win64");
    REQUIRE(win64.Value().script_extension == ".bat");
    REQUIRE(win64.Value().capabilities.dynamic_library);

    auto mobile = Astra::Platform::FindTargetPlatform("android-arm64");
    REQUIRE(mobile);
    REQUIRE_FALSE(mobile.Value().capabilities.dynamic_library);

    auto web = Astra::Platform::FindTargetPlatform("web-wasm32");
    REQUIRE(web);
    REQUIRE(web.Value().architecture == Astra::Platform::TargetArchitecture::Wasm32);
    REQUIRE_FALSE(web.Value().capabilities.threads);

    Astra::Core::DiagnosticSink diagnostics;
    auto unsupported = Astra::Platform::CreatePlatform({Astra::Platform::BackendKind::Web, "web-wasm32"}, diagnostics);
    REQUIRE_FALSE(unsupported);
    REQUIRE(unsupported.Error() == Astra::Core::ErrorCode::Unsupported);
    REQUIRE_FALSE(diagnostics.Diagnostics().empty());
    REQUIRE(diagnostics.Diagnostics().front().code == "ASTRA_PLATFORM_BACKEND_UNSUPPORTED");
}



TEST_CASE("Public headers do not expose forbidden native ABI types") {
    const std::filesystem::path source_root = ASTRA_SOURCE_ROOT;
    const std::vector<std::filesystem::path> public_roots = {
        source_root / "Engine/Runtime/Core/Public",
        source_root / "Engine/Runtime/Platform/Public",
        source_root / "Engine/Runtime/ModuleRuntime/Public",
        source_root / "Engine/Runtime/PropertySystem/Public",
        source_root / "Engine/Runtime/Scene/Public",
        source_root / "Engine/Runtime/Runtime/Public",
        source_root / "Engine/Runtime/Asset/Public",
        source_root / "Engine/Runtime/Media/Public",
        source_root / "Engine/Runtime/Script/Public",
        source_root / "Engine/Runtime/AstraVN/Public",
    };
    const std::vector<std::string> forbidden = {"SDL_", "SDL.h", "SDL_Window", "HWND", "HINSTANCE", "Vk", "ID3D", "ALuint", "bgfx", "Sk", "EditorWidget", "Actor*", "entt::", "entt/"};

    for (const auto& root : public_roots) {
        for (const auto& entry : std::filesystem::recursive_directory_iterator(root)) {
            if (!entry.is_regular_file()) {
                continue;
            }
            std::ifstream file(entry.path(), std::ios::binary);
            const std::string text(std::istreambuf_iterator<char>(file), {});
            for (const auto& token : forbidden) {
                INFO(entry.path().string() << " exposes " << token);
                REQUIRE(text.find(token) == std::string::npos);
            }
        }
    }
}



