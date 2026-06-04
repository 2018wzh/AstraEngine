#pragma once

#include <Astra/Core/Diagnostics.h>
#include <Astra/Core/Result.h>

#include <filesystem>
#include <memory>
#include <optional>
#include <string>

namespace astra {

inline constexpr char kWindowServiceId[] = "astra.platform.window";
inline constexpr char kInputServiceId[] = "astra.platform.input";
inline constexpr char kFileSystemServiceId[] = "astra.platform.filesystem";
inline constexpr char kTimerServiceId[] = "astra.platform.timer";
inline constexpr char kThreadServiceId[] = "astra.platform.thread";
inline constexpr char kDynamicLibraryServiceId[] = "astra.platform.dynamic_library";

enum class PlatformEventType {
    Quit,
};

struct PlatformEvent {
    PlatformEventType type = PlatformEventType::Quit;
};

struct WindowCreateDesc {
    std::string title = "AstraEngine";
    int width = 1280;
    int height = 720;
    bool hidden = false;
};

class IWindow {
  public:
    virtual ~IWindow() = default;
    [[nodiscard]] virtual bool is_open() const = 0;
    [[nodiscard]] virtual std::optional<PlatformEvent> poll_event() = 0;
    virtual void close() = 0;
};

class IWindowService {
  public:
    virtual ~IWindowService() = default;
    [[nodiscard]] virtual Expected<std::unique_ptr<IWindow>>
    create_window(const WindowCreateDesc& desc, DiagnosticSink& diagnostics) = 0;
};

class IInputService {
  public:
    virtual ~IInputService() = default;
    [[nodiscard]] virtual std::string backend_name() const = 0;
};

class IFileSystemService {
  public:
    virtual ~IFileSystemService() = default;
    [[nodiscard]] virtual bool exists(const std::filesystem::path& path) const = 0;
    [[nodiscard]] virtual std::filesystem::path current_path() const = 0;
    [[nodiscard]] virtual std::filesystem::path user_directory() const = 0;
    [[nodiscard]] virtual std::filesystem::path save_directory() const = 0;
    [[nodiscard]] virtual std::filesystem::path cache_directory() const = 0;
};

class ITimerService {
  public:
    virtual ~ITimerService() = default;
    [[nodiscard]] virtual double now_seconds() const = 0;
};

class IThreadService {
  public:
    virtual ~IThreadService() = default;
    [[nodiscard]] virtual unsigned int hardware_concurrency() const = 0;
};

class IDynamicLibrary {
  public:
    virtual ~IDynamicLibrary() = default;
    [[nodiscard]] virtual bool is_loaded() const = 0;
    [[nodiscard]] virtual void* symbol(const char* name) const = 0;
};

class IDynamicLibraryService {
  public:
    virtual ~IDynamicLibraryService() = default;
    [[nodiscard]] virtual Expected<std::unique_ptr<IDynamicLibrary>>
    open_library(const std::filesystem::path& path, DiagnosticSink& diagnostics) const = 0;
};

struct PlatformServicesBundle {
    std::shared_ptr<IWindowService> window;
    std::shared_ptr<IInputService> input;
    std::shared_ptr<IFileSystemService> filesystem;
    std::shared_ptr<ITimerService> timer;
    std::shared_ptr<IThreadService> thread;
    std::shared_ptr<IDynamicLibraryService> dynamic_library;
};

Expected<PlatformServicesBundle> create_default_platform_services(DiagnosticSink& diagnostics);

} // namespace astra
