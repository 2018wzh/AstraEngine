#pragma once

#include <Astra/Core/Diagnostics.hpp>
#include <Astra/Core/Types.hpp>
#include <Astra/Platform/Export.hpp>

#include <chrono>
#include <filesystem>
#include <functional>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

namespace Astra::Platform {

enum class BackendKind {
    Headless,
    Sdl
};

struct WindowDesc {
    std::string title = "AstraEngine";
    Astra::Core::u32 width = 1280;
    Astra::Core::u32 height = 720;
};

class IWindowService {
public:
    virtual ~IWindowService() = default;
    [[nodiscard]] virtual Astra::Core::Result<void> Create(WindowDesc desc, Astra::Core::DiagnosticSink& diagnostics) = 0;
    virtual void PumpEvents() = 0;
    [[nodiscard]] virtual bool ShouldClose() const = 0;
    virtual void Close() = 0;
};

class IFileSystemService {
public:
    virtual ~IFileSystemService() = default;
    [[nodiscard]] virtual Astra::Core::Result<void> Mount(std::string root, std::filesystem::path path, bool read_only) = 0;
    [[nodiscard]] virtual std::filesystem::path Resolve(std::string_view root, std::string_view relative_path) const = 0;
    virtual void Watch(std::filesystem::path path, std::function<void(const std::filesystem::path&)> callback) = 0;
    virtual void PollWatches() = 0;
    [[nodiscard]] virtual Astra::Core::Result<std::string> ReadText(const std::filesystem::path& path) const = 0;
    [[nodiscard]] virtual Astra::Core::Result<void> WriteText(const std::filesystem::path& path, std::string_view text) const = 0;
    [[nodiscard]] virtual bool Exists(const std::filesystem::path& path) const = 0;
};

struct InputSnapshot {
    bool quit_requested = false;
    std::vector<std::string> pressed_keys;
    std::string text_input;
};

class IInputService {
public:
    virtual ~IInputService() = default;
    [[nodiscard]] virtual InputSnapshot Snapshot() const = 0;
    virtual void ResetFrameState() = 0;
};

class ITimerService {
public:
    virtual ~ITimerService() = default;
    [[nodiscard]] virtual Astra::Core::u64 MonotonicNanoseconds() const = 0;
    virtual void SleepFor(std::chrono::milliseconds duration) const = 0;
};

class IThreadService {
public:
    virtual ~IThreadService() = default;
    virtual void Dispatch(std::function<void()> task) = 0;
    virtual void DispatchTagged(std::string tag, std::function<void()> task) = 0;
    virtual void Drain() = 0;
    [[nodiscard]] virtual std::vector<std::string> CompletedTags() const = 0;
    [[nodiscard]] virtual std::vector<std::string> PendingTags() const = 0;
};

struct DynamicLibraryHandle {
    Astra::Core::u64 id = 0;

    [[nodiscard]] bool Empty() const { return id == 0; }
};

class IDynamicLibraryService {
public:
    virtual ~IDynamicLibraryService() = default;
    [[nodiscard]] virtual Astra::Core::Result<DynamicLibraryHandle> Load(const std::filesystem::path& path) = 0;
    [[nodiscard]] virtual Astra::Core::Result<void*> Symbol(DynamicLibraryHandle library, std::string_view name) = 0;
    virtual void Unload(DynamicLibraryHandle library) = 0;
};

class IClipboardService {
public:
    virtual ~IClipboardService() = default;
    [[nodiscard]] virtual std::string GetText() const = 0;
    virtual void SetText(std::string text) = 0;
};

class ICursorService {
public:
    virtual ~ICursorService() = default;
    virtual void SetVisible(bool visible) = 0;
    [[nodiscard]] virtual bool IsVisible() const = 0;
};

struct DisplayInfo {
    Astra::Core::u32 width = 0;
    Astra::Core::u32 height = 0;
    float dpi_scale = 1.0F;
};

class IDisplayService {
public:
    virtual ~IDisplayService() = default;
    [[nodiscard]] virtual std::vector<DisplayInfo> Displays() const = 0;
};

struct CrashPacket {
    std::string build_info;
    std::string minidump_path;
    Astra::Core::u64 frame_index = 0;
    std::string thread_id;
    std::string package_or_project_hash;
    std::vector<Astra::Core::Diagnostic> diagnostics;
    std::vector<std::string> recent_logs;
};

struct CrashCaptureContext {
    std::string build_info;
    std::string minidump_path;
    Astra::Core::u64 frame_index = 0;
    std::string thread_id;
    std::string package_or_project_hash;
    std::vector<std::string> recent_logs;
};

class ASTRA_PLATFORM_API ICrashService {
public:
    virtual ~ICrashService() = default;
    [[nodiscard]] CrashPacket Capture(std::string_view build_info, const Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] virtual CrashPacket Capture(CrashCaptureContext context, const Astra::Core::DiagnosticSink& diagnostics) const = 0;
};

class ASTRA_PLATFORM_API PlatformServices {
public:
    PlatformServices();
    ~PlatformServices();

    PlatformServices(const PlatformServices&) = delete;
    PlatformServices& operator=(const PlatformServices&) = delete;
    PlatformServices(PlatformServices&&) noexcept;
    PlatformServices& operator=(PlatformServices&&) noexcept;

    [[nodiscard]] BackendKind Kind() const;
    [[nodiscard]] IWindowService& Window();
    [[nodiscard]] IFileSystemService& FileSystem();
    [[nodiscard]] IInputService& Input();
    [[nodiscard]] ITimerService& Timer();
    [[nodiscard]] IThreadService& Thread();
    [[nodiscard]] IDynamicLibraryService& DynamicLibrary();
    [[nodiscard]] IClipboardService& Clipboard();
    [[nodiscard]] ICursorService& Cursor();
    [[nodiscard]] IDisplayService& Display();
    [[nodiscard]] ICrashService& Crash();

private:
    struct Impl {
        BackendKind kind = BackendKind::Headless;
        std::unique_ptr<IWindowService> window;
        std::unique_ptr<IFileSystemService> filesystem;
        std::unique_ptr<IInputService> input;
        std::unique_ptr<ITimerService> timer;
        std::unique_ptr<IThreadService> thread;
        std::unique_ptr<IDynamicLibraryService> dynamic_library;
        std::unique_ptr<IClipboardService> clipboard;
        std::unique_ptr<ICursorService> cursor;
        std::unique_ptr<IDisplayService> display;
        std::unique_ptr<ICrashService> crash;
    };
    std::unique_ptr<Impl> impl_;

    friend ASTRA_PLATFORM_API PlatformServices CreateHeadlessPlatform();
    friend ASTRA_PLATFORM_API Astra::Core::Result<PlatformServices> CreateSdlPlatform(Astra::Core::DiagnosticSink& diagnostics);
};

[[nodiscard]] ASTRA_PLATFORM_API PlatformServices CreateHeadlessPlatform();
[[nodiscard]] ASTRA_PLATFORM_API Astra::Core::Result<PlatformServices> CreateSdlPlatform(Astra::Core::DiagnosticSink& diagnostics);

} // namespace Astra::Platform
