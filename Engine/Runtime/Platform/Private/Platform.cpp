#include <Astra/Platform/Platform.hpp>

#include <algorithm>
#include <fstream>
#include <iomanip>
#include <map>
#include <queue>
#include <sstream>
#include <thread>

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#include <Windows.h>
#else
#include <dlfcn.h>
#endif

namespace Astra::Platform {

namespace {

std::string WindowFrameHash(const WindowFrameDesc& frame) {
    constexpr Astra::Core::u64 offset = 14695981039346656037ull;
    constexpr Astra::Core::u64 prime = 1099511628211ull;
    Astra::Core::u64 value = offset;
    auto mix = [&](std::string_view text) {
        for (const auto character : text) {
            value ^= static_cast<unsigned char>(character);
            value *= prime;
        }
    };
    mix(std::to_string(frame.frame_index));
    mix(std::to_string(frame.width));
    mix(std::to_string(frame.height));
    for (const auto& primitive : frame.primitives) {
        mix(primitive.id);
        mix(primitive.kind);
        mix(std::to_string(primitive.x));
        mix(std::to_string(primitive.y));
        mix(std::to_string(primitive.width));
        mix(std::to_string(primitive.height));
        mix(primitive.label);
        mix(std::to_string(primitive.image_width));
        mix(std::to_string(primitive.image_height));
        for (const auto byte : primitive.image_rgba) {
            value ^= byte;
            value *= prime;
        }
    }
    std::ostringstream output;
    output << std::hex << value;
    return output.str();
}

class HeadlessWindowService final : public IWindowService {
public:
    Astra::Core::Result<void> Create(WindowDesc, Astra::Core::DiagnosticSink&) override {
        created_ = true;
        return Astra::Core::Result<void>::Success();
    }

    Astra::Core::Result<WindowPresentEvidence> PresentFrame(const WindowFrameDesc& frame, Astra::Core::DiagnosticSink&) override {
        WindowPresentEvidence evidence;
        evidence.presented = created_;
        evidence.backend = "headless";
        evidence.frame_index = frame.frame_index;
        evidence.primitive_count = static_cast<Astra::Core::u32>(frame.primitives.size());
        evidence.image_primitive_count = static_cast<Astra::Core::u32>(std::ranges::count_if(frame.primitives, [](const WindowFramePrimitive& primitive) {
            return !primitive.image_rgba.empty();
        }));
        evidence.frame_hash = WindowFrameHash(frame);
        return Astra::Core::Result<WindowPresentEvidence>::Success(std::move(evidence));
    }

    void PumpEvents() override {}
    bool ShouldClose() const override { return close_requested_; }
    void Close() override { close_requested_ = true; }

private:
    bool created_ = false;
    bool close_requested_ = false;
};

class FileSystemService final : public IFileSystemService {
public:
    Astra::Core::Result<void> Mount(std::string root, std::filesystem::path path, bool read_only) override {
        mounts_[std::move(root)] = {std::move(path), read_only};
        return Astra::Core::Result<void>::Success();
    }

    std::filesystem::path Resolve(std::string_view root, std::string_view relative_path) const override {
        auto it = mounts_.find(std::string(root));
        if (it == mounts_.end()) {
            return std::filesystem::path(relative_path);
        }
        return (it->second.path / std::filesystem::path(relative_path)).lexically_normal();
    }

    void Watch(std::filesystem::path path, std::function<void(const std::filesystem::path&)> callback) override {
        watches_.push_back({std::move(path), LastWriteTime(path), std::move(callback)});
    }

    void PollWatches() override {
        for (auto& watch : watches_) {
            const auto current = LastWriteTime(watch.path);
            if (current != watch.last_write_time) {
                watch.last_write_time = current;
                watch.callback(watch.path);
            }
        }
    }

    Astra::Core::Result<std::string> ReadText(const std::filesystem::path& path) const override {
        std::ifstream file(path, std::ios::binary);
        if (!file) {
            return Astra::Core::Result<std::string>::Failure(Astra::Core::ErrorCode::NotFound, "file not found");
        }
        return Astra::Core::Result<std::string>::Success(std::string(std::istreambuf_iterator<char>(file), {}));
    }

    Astra::Core::Result<void> WriteText(const std::filesystem::path& path, std::string_view text) const override {
        if (path.has_parent_path()) {
            std::filesystem::create_directories(path.parent_path());
        }
        std::ofstream file(path, std::ios::binary);
        if (!file) {
            return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::PermissionDenied, "cannot write file");
        }
        file.write(text.data(), static_cast<std::streamsize>(text.size()));
        return Astra::Core::Result<void>::Success();
    }

    bool Exists(const std::filesystem::path& path) const override {
        return std::filesystem::exists(path);
    }

private:
    static std::filesystem::file_time_type LastWriteTime(const std::filesystem::path& path) {
        if (!std::filesystem::exists(path)) {
            return {};
        }
        if (std::filesystem::is_directory(path)) {
            std::filesystem::file_time_type latest{};
            for (const auto& entry : std::filesystem::recursive_directory_iterator(path)) {
                if (entry.is_regular_file()) {
                    latest = (std::max)(latest, entry.last_write_time());
                }
            }
            return latest;
        }
        return std::filesystem::last_write_time(path);
    }

    struct MountRecord {
        std::filesystem::path path;
        bool read_only = false;
    };

    struct WatchRecord {
        std::filesystem::path path;
        std::filesystem::file_time_type last_write_time;
        std::function<void(const std::filesystem::path&)> callback;
    };

    std::map<std::string, MountRecord> mounts_;
    std::vector<WatchRecord> watches_;
};

class HeadlessInputService final : public IInputService {
public:
    InputSnapshot Snapshot() const override { return snapshot_; }
    void ResetFrameState() override { snapshot_ = {}; }

private:
    InputSnapshot snapshot_;
};

class TimerService final : public ITimerService {
public:
    Astra::Core::u64 MonotonicNanoseconds() const override {
        return static_cast<Astra::Core::u64>(
            std::chrono::duration_cast<std::chrono::nanoseconds>(std::chrono::steady_clock::now().time_since_epoch()).count());
    }

    void SleepFor(std::chrono::milliseconds duration) const override {
        std::this_thread::sleep_for(duration);
    }
};

class ThreadService final : public IThreadService {
public:
    void Dispatch(std::function<void()> task) override {
        DispatchTagged("default", std::move(task));
    }

    void DispatchTagged(std::string tag, std::function<void()> task) override {
        tasks_.push({std::move(tag), std::move(task)});
    }

    void Drain() override {
        while (!tasks_.empty()) {
            auto task = std::move(tasks_.front());
            tasks_.pop();
            task.callback();
            completed_tags_.push_back(std::move(task.tag));
        }
    }

    std::vector<std::string> CompletedTags() const override { return completed_tags_; }
    std::vector<std::string> PendingTags() const override {
        auto copy = tasks_;
        std::vector<std::string> tags;
        while (!copy.empty()) {
            tags.push_back(copy.front().tag);
            copy.pop();
        }
        return tags;
    }

private:
    struct TaggedTask {
        std::string tag;
        std::function<void()> callback;
    };

    std::queue<TaggedTask> tasks_;
    std::vector<std::string> completed_tags_;
};

class DynamicLibraryService final : public IDynamicLibraryService {
public:
    Astra::Core::Result<DynamicLibraryHandle> Load(const std::filesystem::path& path) override {
#if defined(_WIN32)
        auto* handle = reinterpret_cast<void*>(LoadLibraryW(path.wstring().c_str()));
#else
        auto* handle = dlopen(path.string().c_str(), RTLD_NOW);
#endif
        if (handle == nullptr) {
            return Astra::Core::Result<DynamicLibraryHandle>::Failure(Astra::Core::ErrorCode::NotFound, "dynamic library could not be loaded");
        }
        const auto id = next_id_++;
        libraries_[id] = handle;
        return Astra::Core::Result<DynamicLibraryHandle>::Success({id});
    }

    Astra::Core::Result<void*> Symbol(DynamicLibraryHandle library, std::string_view name) override {
        auto it = libraries_.find(library.id);
        if (library.Empty() || it == libraries_.end()) {
            return Astra::Core::Result<void*>::Failure(Astra::Core::ErrorCode::InvalidArgument, "library handle is null");
        }
        std::string symbol_name(name);
#if defined(_WIN32)
        auto* symbol = reinterpret_cast<void*>(GetProcAddress(static_cast<HMODULE>(it->second), symbol_name.c_str()));
#else
        auto* symbol = dlsym(it->second, symbol_name.c_str());
#endif
        if (symbol == nullptr) {
            return Astra::Core::Result<void*>::Failure(Astra::Core::ErrorCode::NotFound, "dynamic library symbol not found");
        }
        return Astra::Core::Result<void*>::Success(symbol);
    }

    void Unload(DynamicLibraryHandle library) override {
        auto it = libraries_.find(library.id);
        if (library.Empty() || it == libraries_.end()) {
            return;
        }
#if defined(_WIN32)
        FreeLibrary(static_cast<HMODULE>(it->second));
#else
        dlclose(it->second);
#endif
        libraries_.erase(it);
    }

private:
    Astra::Core::u64 next_id_ = 1;
    std::map<Astra::Core::u64, void*> libraries_;
};

class ClipboardService final : public IClipboardService {
public:
    std::string GetText() const override { return text_; }
    void SetText(std::string text) override { text_ = std::move(text); }

private:
    std::string text_;
};

class CursorService final : public ICursorService {
public:
    void SetVisible(bool visible) override { visible_ = visible; }
    bool IsVisible() const override { return visible_; }

private:
    bool visible_ = true;
};

class DisplayService final : public IDisplayService {
public:
    std::vector<DisplayInfo> Displays() const override { return {{0, 0, 1.0F}}; }
};

class CrashService final : public ICrashService {
public:
    CrashPacket Capture(CrashCaptureContext context, const Astra::Core::DiagnosticSink& diagnostics) const override {
        CrashPacket packet;
        packet.build_info = std::move(context.build_info);
        packet.minidump_path = std::move(context.minidump_path);
        packet.frame_index = context.frame_index;
        packet.thread_id = context.thread_id.empty() ? CurrentThreadId() : std::move(context.thread_id);
        packet.package_or_project_hash = std::move(context.package_or_project_hash);
        packet.recent_logs = std::move(context.recent_logs);
        packet.diagnostics = diagnostics.Diagnostics();
        return packet;
    }

private:
    static std::string CurrentThreadId() {
        std::ostringstream stream;
        stream << std::this_thread::get_id();
        return stream.str();
    }
};

} // namespace

PlatformServices::PlatformServices() : impl_(std::make_unique<Impl>()) {}
PlatformServices::~PlatformServices() = default;
PlatformServices::PlatformServices(PlatformServices&&) noexcept = default;
PlatformServices& PlatformServices::operator=(PlatformServices&&) noexcept = default;

BackendKind PlatformServices::Kind() const { return impl_->kind; }
IWindowService& PlatformServices::Window() { return *impl_->window; }
IFileSystemService& PlatformServices::FileSystem() { return *impl_->filesystem; }
IInputService& PlatformServices::Input() { return *impl_->input; }
ITimerService& PlatformServices::Timer() { return *impl_->timer; }
IThreadService& PlatformServices::Thread() { return *impl_->thread; }
IDynamicLibraryService& PlatformServices::DynamicLibrary() { return *impl_->dynamic_library; }
IClipboardService& PlatformServices::Clipboard() { return *impl_->clipboard; }
ICursorService& PlatformServices::Cursor() { return *impl_->cursor; }
IDisplayService& PlatformServices::Display() { return *impl_->display; }
ICrashService& PlatformServices::Crash() { return *impl_->crash; }

CrashPacket ICrashService::Capture(std::string_view build_info, const Astra::Core::DiagnosticSink& diagnostics) const {
    CrashCaptureContext context;
    context.build_info = std::string(build_info);
    return Capture(std::move(context), diagnostics);
}

PlatformServices CreateHeadlessPlatform() {
    PlatformServices services;
    services.impl_->kind = BackendKind::Headless;
    services.impl_->window = std::make_unique<HeadlessWindowService>();
    services.impl_->filesystem = std::make_unique<FileSystemService>();
    services.impl_->input = std::make_unique<HeadlessInputService>();
    services.impl_->timer = std::make_unique<TimerService>();
    services.impl_->thread = std::make_unique<ThreadService>();
    services.impl_->dynamic_library = std::make_unique<DynamicLibraryService>();
    services.impl_->clipboard = std::make_unique<ClipboardService>();
    services.impl_->cursor = std::make_unique<CursorService>();
    services.impl_->display = std::make_unique<DisplayService>();
    services.impl_->crash = std::make_unique<CrashService>();
    return services;
}

} // namespace Astra::Platform
