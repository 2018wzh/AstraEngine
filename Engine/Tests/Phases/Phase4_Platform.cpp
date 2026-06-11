TEST_CASE("Headless platform provides filesystem timer thread and crash services") {
    auto platform = Astra::Platform::CreateHeadlessPlatform();
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
    const std::vector<std::string> forbidden = {"SDL_", "SDL.h", "HWND", "HINSTANCE", "Vk", "ID3D", "ALuint", "EditorWidget", "Actor*", "entt::", "entt/"};

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



