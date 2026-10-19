#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include "astra_emu.h"
#include "NativeSession.h"
#include "NativeLog.h"
#include <spdlog/spdlog.h>
#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cmath>
#include <condition_variable>
#include <cstring>
#include <filesystem>
#include <memory>
#include <mutex>
#include <stdexcept>
#include <string>
#include <string_view>
#include <thread>
#include <vector>

namespace {
using Clock = std::chrono::steady_clock;
std::atomic<bool> runtime_poisoned{false};
template<size_t N> AstraEmuBytes literal(const char (&s)[N]) { return {reinterpret_cast<const uint8_t *>(s), N - 1}; }
struct Failure : std::runtime_error {
    AstraEmuStatus status;
    AstraEmuError detail{};
    bool has_detail = false;
    Failure(AstraEmuStatus s, const char *message) : std::runtime_error(message), status(s) {}
    Failure(AstraEmuStatus s, const AstraEmuError &error)
        : std::runtime_error(std::string(reinterpret_cast<const char *>(error.message),
              std::min<size_t>(error.message_len, sizeof(error.message)))),
          status(s), detail(error), has_detail(true) {}
};
void require(bool condition, const char *code, AstraEmuStatus status = AstraEmuINVALID_ARGUMENT) {
    if(!condition) throw Failure(status, code);
}
void error(AstraEmuError *out, const char *message) noexcept {
    if(!out) return;
    *out = {};
    const auto n = std::min(std::strlen(message), sizeof(out->message));
    std::memcpy(out->message, message, n); out->message_len = static_cast<uint32_t>(n);
    constexpr char code[] = "KRKR_NATIVE";
    std::memcpy(out->code, code, sizeof(code) - 1); out->code_len = sizeof(code) - 1;
}
template<class F> AstraEmuStatus boundary(AstraEmuError *out, F call) noexcept {
    if(out) *out = {};
    try { call(); return AstraEmuOK; }
    catch(const NativeCancelled &e) { error(out, e.what()); return AstraEmuCANCELLED; }
    catch(const Failure &e) {
        if(out && e.has_detail) *out = e.detail; else error(out, e.what());
        return e.status;
    }
    catch(const std::exception &e) { error(out, e.what()); return AstraEmuCORE_ERROR; }
    catch(...) { error(out, "KRKR_NATIVE_EXCEPTION"); return AstraEmuPANICKED; }
}
std::string text(AstraEmuBytes value, size_t maximum = 4096) {
    require(value.len <= maximum && (!value.len || value.data), "KRKR_TEXT_BOUND");
    if(!value.len) return {};
    require(!std::memchr(value.data, 0, value.len), "KRKR_TEXT_NUL");
    require(MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, reinterpret_cast<const char *>(value.data), static_cast<int>(value.len), nullptr, 0) != 0, "KRKR_TEXT_UTF8");
    return {reinterpret_cast<const char *>(value.data), value.len};
}
AstraEmuSurfaceInfo surface(AstraKrkrFrameInfo f) {
    // TVPWindowLayer::UpdateDrawBuffer presents this same final texture with
    // BlendFunc::DISABLE. Preserve that RGB presentation, not layer compositing.
    return {AstraEmuSURFACE_CPU, AstraEmuPIXEL_RGBA8, AstraEmuCOLOR_SRGB,
        AstraEmuALPHA_IGNORED, AstraEmuROWS_TOP_DOWN,
        {f.logical_width, f.logical_height}, {f.width, f.height}, f.stride};
}
struct Frames;
struct Slot {
    Frames *pool;
    std::atomic<unsigned> leases{0};
    std::unique_ptr<uint8_t[]> pixels;
    size_t capacity = 0, length = 0;
    AstraEmuFrameIdentity identity{};
    AstraEmuSurfaceInfo info{};
    uint64_t timestamp = 0;
};
struct Frames {
    std::atomic<unsigned> owners{1};
    std::array<Slot, 3> slots;
    Frames() { for(auto &slot : slots) slot.pool = this; }
    void release() { if(owners.fetch_sub(1, std::memory_order_acq_rel) == 1) delete this; }
};
struct FramesRelease { void operator()(Frames *pool) const noexcept { pool->release(); } };
void release_frame(void *owner) noexcept {
    auto slot = static_cast<Slot *>(owner);
    auto pool = slot->pool;
    slot->leases.fetch_sub(1, std::memory_order_release);
    pool->release();
}
struct Input { AstraKrkrEvent event{}; std::string characters; };
struct Session {
    const uint64_t epoch;
    AstraEmuHostServices host;
    AstraEmuWindowState window;
    std::string game, resources, writable;
    std::thread worker;
    std::mutex mutex;
    std::condition_variable wake;
    std::atomic<bool> cancelled{false};
    std::atomic<uint32_t> phase{AstraEmuSESSION_RUNNING};
    bool opened = false, ended = false;
    AstraEmuStatus failure = AstraEmuOK;
    AstraEmuError failure_error{};
    AstraEmuStatus host_failure = AstraEmuOK;
    AstraEmuError host_failure_error{};
    std::vector<Input> input;
    std::array<bool, 96> held_keys{};
    std::unique_ptr<Frames, FramesRelease> frames{new Frames};
    Slot *latest = nullptr;
    uint64_t output_epoch = 1, generation = 0;
    std::atomic<uint64_t> request_id{0};
    Session(const AstraEmuOpenRequest &r) : epoch(r.session_epoch), host(r.host), window(r.window) { input.reserve(256); }
    bool stopping() const {
        return cancelled.load(std::memory_order_acquire) || host.cancellation.is_cancelled(host.cancellation.context, epoch) != 0;
    }
    void cancel() noexcept {
        if(!cancelled.exchange(true, std::memory_order_acq_rel)) {
            if(host.window.invalidate) host.window.invalidate(host.window.context, epoch, 1);
            if(host.text.invalidate) host.text.invalidate(host.text.context, epoch, 1);
        }
        wake.notify_all();
    }
    static uint32_t is_cancelled(void *p) { return static_cast<Session *>(p)->stopping(); }
    void remember_host_failure(AstraEmuStatus status, const AstraEmuError &detail) {
        std::lock_guard<std::mutex> lock(mutex);
        if(host_failure == AstraEmuOK) {
            host_failure = status;
            host_failure_error = detail;
        }
    }
    void begin_native_operation() {
        std::lock_guard<std::mutex> lock(mutex);
        host_failure = AstraEmuOK;
    }
    void check_host_result(AstraEmuStatus status, AstraEmuError detail, const char *fallback) {
        if(status == AstraEmuOK) return;
        if(!detail.code_len && !detail.message_len) error(&detail, fallback);
        remember_host_failure(status, detail);
        throw Failure(status, detail);
    }
    static int32_t modal(void *p, const AstraKrkrModalRequest *r, AstraKrkrModalResponse *out) {
        auto &s = *static_cast<Session *>(p);
        try {
            require(!s.stopping(), "KRKR_CANCELLED", AstraEmuCANCELLED);
            std::array<AstraEmuWindowButton, 16> buttons{};
            for(uint32_t i = 0; i < r->button_count; ++i)
                buttons[i] = {i, 0, {r->buttons[i].data, static_cast<size_t>(r->buttons[i].bytes)}};
            AstraEmuWindowRequest request{};
            request.session_epoch = s.epoch; request.context_epoch = 1; request.request_id = ++s.request_id;
            request.kind = AstraEmuWINDOW_MODAL;
            request.flags = r->has_input ? AstraEmuMODAL_TEXT_INPUT : 0;
            request.caption = {r->caption.data, static_cast<size_t>(r->caption.bytes)};
            request.text = {r->message.data, static_cast<size_t>(r->message.bytes)};
            request.initial_text = {r->initial_text.data, static_cast<size_t>(r->initial_text.bytes)};
            request.buttons = buttons.data(); request.button_count = r->button_count;
            spdlog::info("KRKR_HOST_MODAL request_id={} caption={} message={}", request.request_id,
                std::string_view(reinterpret_cast<const char *>(r->caption.data), r->caption.bytes),
                std::string_view(reinterpret_cast<const char *>(r->message.data), r->message.bytes));
            require(s.host.window.request, "KRKR_MODAL_SERVICE_REQUIRED", AstraEmuUNSUPPORTED);
            AstraEmuWindowResult result{};
            result.input = {out->text, static_cast<size_t>(out->text_capacity), 0};
            AstraEmuError e{};
            s.phase.store(AstraEmuSESSION_WAITING);
            const auto status = s.host.window.request(s.host.window.context, &request, &result, &e);
            s.phase.store(AstraEmuSESSION_RUNNING);
            s.check_host_result(status, e, "KRKR_MODAL_FAILED");
            require(!s.stopping(), "KRKR_CANCELLED", AstraEmuCANCELLED);
            require((result.valid_fields & AstraEmuWINDOW_RESULT_MODAL) && result.selected_button < r->button_count &&
                result.input.data == out->text && result.input.capacity == out->text_capacity && result.input.len <= out->text_capacity,
                "KRKR_MODAL_RESPONSE");
            out->kind = 1; out->button_index = result.selected_button; out->text_bytes = result.input.len;
            return 0;
        } catch(const Failure &e) {
            AstraEmuError detail{};
            if(e.has_detail) detail = e.detail; else error(&detail, e.what());
            s.remember_host_failure(e.status, detail);
            return e.status;
        }
        catch(...) { return AstraEmuCORE_ERROR; }
    }
    void publish(astra::krkr::NativeSession &native) {
        Slot *target = nullptr;
        {
            std::lock_guard<std::mutex> lock(mutex);
            for(auto &slot : frames->slots)
                if(&slot != latest && slot.leases.load(std::memory_order_acquire) == 0) {
                    target = &slot;
                    slot.identity = {}; // No input may borrow this slot during readback.
                    break;
                }
        }
        if(!target) return; // Bounded backpressure; dirty canvas remains pending.
        const auto info = native.frame_info();
        target->length = static_cast<size_t>(info.stride) * info.height;
        if(target->capacity < target->length) {
            target->pixels.reset(new uint8_t[target->length]);
            target->capacity = target->length;
        }
        native.copy_frame(info, target->pixels.get(), target->length);
        target->info = surface(info);
        target->timestamp = std::chrono::duration_cast<std::chrono::nanoseconds>(Clock::now().time_since_epoch()).count();
        std::lock_guard<std::mutex> lock(mutex);
        if(latest && std::memcmp(&latest->info, &target->info, sizeof(target->info))) ++output_epoch;
        target->identity = {epoch, output_epoch, ++generation};
        latest = target;
    }
    AstraEmuWindowResult window_request(uint32_t kind, uint32_t flags, AstraEmuBytes text) {
        require(!stopping(), "KRKR_CANCELLED", AstraEmuCANCELLED);
        if(!host.window.request)
            check_host_result(AstraEmuUNSUPPORTED, {}, "KRKR_WINDOW_SERVICE_REQUIRED");
        AstraEmuWindowRequest r{};
        r.session_epoch = epoch; r.context_epoch = 1; r.request_id = ++request_id;
        r.kind = kind; r.flags = flags; r.text = text;
        AstraEmuWindowResult result{}; AstraEmuError e{};
        const auto status = host.window.request(host.window.context, &r, &result, &e);
        check_host_result(status, e, "KRKR_WINDOW_REQUEST_FAILED");
        return result;
    }
    static bool fullscreen(void *p, bool enabled) {
        auto &s = *static_cast<Session *>(p);
        const auto result = s.window_request(AstraEmuWINDOW_FULLSCREEN,
            enabled ? AstraEmuFULLSCREEN_BORDERLESS : AstraEmuFULLSCREEN_WINDOWED, {});
        require((result.valid_fields & AstraEmuWINDOW_RESULT_MODE) &&
            result.fullscreen <= AstraEmuFULLSCREEN_EXCLUSIVE, "KRKR_WINDOW_MODE_RESPONSE");
        return result.fullscreen != AstraEmuFULLSCREEN_WINDOWED;
    }
    static void caption(void *p, const std::string &value) {
        static_cast<Session *>(p)->window_request(AstraEmuWINDOW_TITLE, 0,
            {reinterpret_cast<const uint8_t *>(value.data()), value.size()});
    }
    void run() noexcept {
        AstraKrkrBootOptions options{reinterpret_cast<const uint8_t *>(game.data()), game.size(),
            reinterpret_cast<const uint8_t *>(resources.data()), resources.size(),
            reinterpret_cast<const uint8_t *>(writable.data()), writable.size(),
            {window.drawable_width, window.drawable_height, window.focused, 1}, {this, is_cancelled, modal, fullscreen, caption}};
        std::unique_ptr<astra::krkr::NativeSession> native;
        AstraEmuError e{};
        auto status = boundary(&e, [&] {
            begin_native_operation();
            require(!stopping(), "KRKR_CANCELLED", AstraEmuCANCELLED);
            native = std::make_unique<astra::krkr::NativeSession>(options);
            native->boot(options);
            require(!stopping(), "KRKR_CANCELLED", AstraEmuCANCELLED);
            if(window.suspended) {
                AstraKrkrEvent suspend{}; suspend.kind = 9; suspend.value = 1;
                native->advance(0, &suspend, 1);
                phase.store(AstraEmuSESSION_PAUSED);
            }
            publish(*native);
            {
                std::lock_guard<std::mutex> lock(mutex); opened = true;
            }
            wake.notify_all();
            auto previous = Clock::now();
            auto previous_wait = native->modal_wait_ns();
            std::vector<Input> pending; pending.reserve(256);
            std::vector<AstraKrkrEvent> events; events.reserve(256);
            while(!stopping()) {
                {
                    std::unique_lock<std::mutex> lock(mutex);
                    wake.wait_for(lock, std::chrono::nanoseconds(native->frame_interval_ns()), [&] { return stopping() || !input.empty(); });
                    input.swap(pending);
                }
                events.clear();
                for(auto &item : pending) {
                    item.event.text_utf8 = reinterpret_cast<const uint8_t *>(item.characters.data());
                    item.event.text_bytes = item.characters.size();
                    events.push_back(item.event);
                }
                const auto now = Clock::now();
                const auto waiting = native->modal_wait_ns();
                const auto elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(now - previous).count();
                const auto active_elapsed = elapsed - static_cast<int64_t>(waiting - previous_wait);
                previous = now;
                previous_wait = waiting;
                // A script may handle a failed request. Only retain its first
                // cause for the current native operation, not subsequent ticks.
                begin_native_operation();
                const auto result = native->advance(static_cast<uint64_t>(std::max<int64_t>(0, active_elapsed)), events.data(), static_cast<uint32_t>(events.size()));
                pending.clear();
                if(result.reset_clock) previous = Clock::now();
                if(result.state == 2) {
                    spdlog::info("KRKR_FINISHED exit_code={}", result.exit_code);
                    phase.store(AstraEmuSESSION_FINISHED);
                    break;
                }
                if(result.frame_changed) publish(*native);
            }
        });
        if(status != AstraEmuOK && status != AstraEmuCANCELLED) {
            std::lock_guard<std::mutex> lock(mutex);
            if(host_failure != AstraEmuOK) {
                status = host_failure;
                e = host_failure_error;
            }
        }
        if(native) {
            AstraEmuError cleanup_error{};
            const auto cleanup_status = boundary(&cleanup_error, [&] { native->shutdown(); });
            native.reset();
            if(cleanup_status != AstraEmuOK) {
                runtime_poisoned.store(true);
                if(status == AstraEmuOK || status == AstraEmuCANCELLED) {
                    status = cleanup_status; e = cleanup_error;
                }
            }
        }
        std::error_code cleanup;
        if(!writable.empty()) std::filesystem::remove_all(std::filesystem::u8path(writable), cleanup);
        if(cleanup && status == AstraEmuOK) { status = AstraEmuCORE_ERROR; error(&e, "KRKR_WRITABLE_CLEANUP"); }
        {
            std::lock_guard<std::mutex> lock(mutex);
            failure = status; failure_error = e; ended = true;
            if(status != AstraEmuOK) phase.store(AstraEmuSESSION_FAILED);
        }
        wake.notify_all();
    }
};
std::atomic<bool> occupied{false};
std::atomic<uint64_t> temporary_id{0};
Session &session(AstraEmuSession handle) { require(handle, "KRKR_SESSION_NULL"); return *static_cast<Session *>(handle); }
void window_state(const AstraEmuWindowState &w) {
    require(w.drawable_width && w.drawable_height && std::isfinite(w.scale_factor) && w.scale_factor > 0 && w.focused <= 1 && w.suspended <= 1, "KRKR_WINDOW_STATE");
}
uint32_t key(const std::string &name) {
    if(name.size() == 4 && name.compare(0, 3, "Key") == 0 && name[3] >= 'A' && name[3] <= 'Z') return name[3] - 'A' + 1;
    if(name.size() == 6 && name.compare(0, 5, "Digit") == 0 && name[5] >= '0' && name[5] <= '9') return name[5] - '0' + 27;
    static constexpr const char *names[] = {"Escape","Enter","Tab","Backspace","Space","Insert","Delete","Home","End","PageUp","PageDown","ArrowLeft","ArrowRight","ArrowUp","ArrowDown","ControlLeft","ControlRight","ShiftLeft","ShiftRight","AltLeft","AltRight","F1","F2","F3","F4","F5","F6","F7","F8","F9","F10","F11","F12","Numpad0","Numpad1","Numpad2","Numpad3","Numpad4","Numpad5","Numpad6","Numpad7","Numpad8","Numpad9","NumpadEnter","NumpadAdd","NumpadSubtract","NumpadMultiply","NumpadDivide","Minus","Equal","Comma","Period","Slash","Semicolon","Quote","BracketLeft","BracketRight","Backslash","Backquote"};
    for(uint32_t i = 0; i < std::size(names); ++i) if(name == names[i]) return i + 37;
    throw Failure(AstraEmuUNSUPPORTED, "KRKR_PHYSICAL_KEY_UNSUPPORTED");
}
AstraEmuStatus initialize(const AstraEmuDiagnostics *d, AstraEmuError *e) noexcept {
    return boundary(e, [&] {
        require(d && d->header.size >= sizeof(*d) && d->header.version == AstraEmuABI_VERSION, "KRKR_DIAGNOSTICS_HEADER");
        require(!occupied.load(), "KRKR_INITIALIZE_ACTIVE", AstraEmuINVALID_STATE);
        auto sink = std::make_shared<astra::krkr::NativeLog>(*d);
        for(const char *name : {"core", "tjs2", "plugin"}) {
            if(auto logger = spdlog::get(name)) logger->sinks().assign(1, sink);
            else spdlog::register_logger(std::make_shared<spdlog::logger>(name, sink));
        }
        spdlog::set_default_logger(spdlog::get("core"));
        spdlog::set_level(spdlog::level::debug);
    });
}
AstraEmuStatus describe(AstraEmuDescriptor *out, AstraEmuError *e) noexcept {
    return boundary(e, [&] {
        require(out, "KRKR_DESCRIPTOR_NULL");
        static const AstraEmuBytes formats[] = {literal("xp3"), literal("tjs")};
        *out = {{sizeof(*out), AstraEmuABI_VERSION}, literal("krkr"), literal("astra-emu-krkr"), literal("0.1.0"), AstraEmuCAP_CPU_SURFACE, formats, 2, nullptr, 0,
            literal("KiriKiri2"), literal("KiriKiri"), {}};
    });
}
AstraEmuStatus probe(AstraEmuBytes path, AstraEmuProbeResult *out, AstraEmuError *e) noexcept {
    if(out) *out = {};
    return boundary(e, [&] {
        require(out && path.len, "KRKR_PROBE_ARGUMENT");
        const auto p = std::filesystem::absolute(std::filesystem::u8path(text(path)));
        const auto directory = std::filesystem::is_directory(p) ? p : p.parent_path();
        const bool loose = std::filesystem::is_regular_file(directory / "startup.tjs");
        require(loose || std::filesystem::is_regular_file(directory / "data.xp3"), "KRKR_NO_MATCH", AstraEmuNO_MATCH);
        static std::string identity;
        identity = directory.lexically_normal().u8string();
        *out = {{reinterpret_cast<const uint8_t *>(identity.data()), identity.size()},
            loose ? literal("tjs") : literal("xp3"), 9500, 0};
    });
}
AstraEmuStatus open(const AstraEmuOpenRequest *r, AstraEmuOpenResult *out, AstraEmuError *e) noexcept {
    if(out) *out = {};
    return boundary(e, [&] {
        require(r && out && r->header.size >= sizeof(*r) && r->header.version == AstraEmuABI_VERSION && r->session_epoch, "KRKR_OPEN_HEADER");
        require(r->game_path.len, "KRKR_GAME_PATH_EMPTY");
        require(r->host.header.size >= sizeof(r->host) && r->host.header.version == AstraEmuABI_VERSION && r->host.cancellation.is_cancelled, "KRKR_HOST_HEADER");
        require(!r->host.window.request || r->host.window.invalidate, "KRKR_WINDOW_CANCELLATION_REQUIRED");
        require(!r->configuration_count, "KRKR_CONFIGURATION_UNSUPPORTED", AstraEmuUNSUPPORTED);
        require(!r->render_extent.width && !r->render_extent.height, "KRKR_RENDER_EXTENT_UNSUPPORTED");
        window_state(r->window);
        require(!runtime_poisoned.load(), "KRKR_PREVIOUS_TEARDOWN_FAILED", AstraEmuINVALID_STATE);
        require(!occupied.exchange(true), "KRKR_SESSION_ACTIVE", AstraEmuINVALID_STATE);
        std::unique_ptr<Session> s;
        try {
            s = std::make_unique<Session>(*r);
            auto path = std::filesystem::absolute(std::filesystem::u8path(text(r->game_path)));
            if(!std::filesystem::is_directory(path)) path = path.parent_path();
            s->game = path.u8string();
            HMODULE module = nullptr; wchar_t filename[32768];
            require(GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                reinterpret_cast<LPCWSTR>(&open), &module), "KRKR_MODULE_PATH", AstraEmuCORE_ERROR);
            const auto n = GetModuleFileNameW(module, filename, static_cast<DWORD>(std::size(filename)));
            require(n && n < std::size(filename), "KRKR_MODULE_PATH", AstraEmuCORE_ERROR);
            s->resources = (std::filesystem::path(filename).parent_path() / "Resources").u8string();
            const auto temporary = std::filesystem::temp_directory_path() / ("astra-krkr-" + std::to_string(GetCurrentProcessId()) + "-" + std::to_string(++temporary_id));
            require(std::filesystem::create_directory(temporary), "KRKR_WRITABLE_CREATE", AstraEmuCORE_ERROR);
            s->writable = temporary.u8string();
            s->worker = std::thread([p = s.get()] { p->run(); });
            std::unique_lock<std::mutex> lock(s->mutex);
            while(!s->opened && !s->ended) {
                s->wake.wait_for(lock, std::chrono::milliseconds(10));
                if(s->stopping()) { lock.unlock(); s->cancel(); lock.lock(); }
            }
            if(s->failure != AstraEmuOK) throw Failure(s->failure, s->failure_error);
            require(!s->stopping(), "KRKR_CANCELLED", AstraEmuCANCELLED);
            require(s->latest, "KRKR_CANVAS_MISSING", AstraEmuCORE_ERROR);
            out->context_epoch = 1; out->output_epoch = s->output_epoch; out->surface = s->latest->info;
            lock.unlock(); out->session = s.release();
        } catch(...) {
            if(s) {
                s->cancel();
                if(s->worker.joinable()) s->worker.join();
                else if(!s->writable.empty()) std::filesystem::remove_all(std::filesystem::u8path(s->writable));
            }
            occupied.store(false);
            throw;
        }
    });
}
AstraEmuStatus input(AstraEmuSession handle, const AstraEmuInputEvent *events, size_t count, AstraEmuError *e) noexcept {
    return boundary(e, [&] {
        auto &s = session(handle);
        require(count <= 4096 && (!count || events), "KRKR_INPUT_BOUND");
        std::lock_guard<std::mutex> lock(s.mutex);
        require(!s.ended && !s.stopping(), "KRKR_SESSION_STOPPED", AstraEmuINVALID_STATE);
        require(s.input.size() + count * 2 <= 4096, "KRKR_INPUT_CAPACITY", AstraEmuCAPACITY);
        for(size_t i = 0; i < count; ++i) {
            const auto &v = events[i]; Input item;
            require(v.pressed <= 1 && v.repeat <= 1 && std::isfinite(v.x) && std::isfinite(v.y), "KRKR_INPUT_VALUE");
            if(v.kind == AstraEmuINPUT_KEY) {
                item.event.kind = 1; item.event.key_code = key(text(v.physical_key, 64));
                item.event.key_state = !v.pressed ? 1 : v.repeat ? 2 : 0;
                const auto code = item.event.key_code;
                s.held_keys[code] = v.pressed != 0;
                item.event.modifiers = (s.held_keys[52] || s.held_keys[53] ? 2u : 0u) |
                    (s.held_keys[54] || s.held_keys[55] ? 1u : 0u) |
                    (s.held_keys[56] || s.held_keys[57] ? 4u : 0u);
                s.input.push_back(std::move(item));
                const auto logical = text(v.logical_key, 64);
                if(v.pressed && !logical.empty() && (logical.size() == 1 || static_cast<unsigned char>(logical[0]) >= 128) && static_cast<unsigned char>(logical[0]) >= 32) {
                    Input typed; typed.event.kind = 5; typed.characters = logical; s.input.push_back(std::move(typed));
                }
                continue;
            } else if(v.kind == AstraEmuINPUT_WINDOW) {
                window_state(v.window);
                item.event.kind = 6; item.event.value = v.window.focused;
                s.input.push_back(std::move(item));
                item = {}; item.event.kind = 9; item.event.value = v.window.suspended;
                if(!v.window.focused || v.window.suspended) s.held_keys.fill(false);
            } else if(v.kind == AstraEmuINPUT_POINTER_MOVE || v.kind == AstraEmuINPUT_POINTER_BUTTON) {
                Slot *displayed = nullptr;
                for(auto &slot : s.frames->slots)
                    if(slot.identity.session_epoch == v.displayed.session_epoch && slot.identity.output_epoch == v.displayed.output_epoch &&
                        slot.identity.generation == v.displayed.generation && slot.identity.generation) { displayed = &slot; break; }
                require(displayed && v.displayed.output_epoch == s.output_epoch, "KRKR_INPUT_STALE_FRAME", AstraEmuSTALE_CONTEXT);
                item.event.kind = 2;
                item.event.x = static_cast<float>(v.x * displayed->info.logical.width / displayed->info.raster.width);
                item.event.y = static_cast<float>(v.y * displayed->info.logical.height / displayed->info.raster.height);
                require(std::isfinite(item.event.x) && std::isfinite(item.event.y) &&
                    static_cast<double>(item.event.x) >= INT32_MIN && static_cast<double>(item.event.x) <= INT32_MAX &&
                    static_cast<double>(item.event.y) >= INT32_MIN && static_cast<double>(item.event.y) <= INT32_MAX, "KRKR_POINTER_RANGE");
                if(v.kind == AstraEmuINPUT_POINTER_BUTTON) {
                    require(v.button >= 1 && v.button <= 5, "KRKR_POINTER_BUTTON", AstraEmuUNSUPPORTED);
                    s.input.push_back(item); item.event.kind = 3; item.event.pointer_button = v.button - 1; item.event.key_state = v.pressed ? 0 : 1;
                }
            } else if(v.kind == AstraEmuINPUT_WHEEL) {
                require(v.x == 0, "KRKR_HORIZONTAL_WHEEL_UNSUPPORTED", AstraEmuUNSUPPORTED);
                require(std::abs(v.y) <= static_cast<double>(INT32_MAX) / 120.0, "KRKR_WHEEL_RANGE");
                item.event.kind = 4; item.event.y = static_cast<float>(v.y);
            } else throw Failure(AstraEmuUNSUPPORTED, "KRKR_INPUT_KIND");
            s.input.push_back(std::move(item));
        }
        s.wake.notify_all();
    });
}
AstraEmuStatus control(AstraEmuSession handle, uint32_t command, AstraEmuError *e) noexcept {
    return boundary(e, [&] {
        auto &s = session(handle);
        require(command == AstraEmuCONTROL_PAUSE || command == AstraEmuCONTROL_RESUME, "KRKR_CONTROL_UNSUPPORTED", AstraEmuUNSUPPORTED);
        std::lock_guard<std::mutex> lock(s.mutex);
        require(!s.ended && s.input.size() < 4096, "KRKR_CONTROL_STATE", AstraEmuINVALID_STATE);
        Input item; item.event.kind = 9; item.event.value = command == AstraEmuCONTROL_PAUSE;
        if(command == AstraEmuCONTROL_PAUSE) s.held_keys.fill(false);
        s.input.push_back(std::move(item));
        s.phase.store(command == AstraEmuCONTROL_PAUSE ? AstraEmuSESSION_PAUSED : AstraEmuSESSION_RUNNING);
        s.wake.notify_all();
    });
}
AstraEmuStatus state(AstraEmuSession handle, AstraEmuSessionState *out, AstraEmuError *e) noexcept {
    if(out) *out = {};
    return boundary(e, [&] { auto &s = session(handle); require(out, "KRKR_STATE_NULL"); std::lock_guard<std::mutex> lock(s.mutex); *out = {s.phase.load(), 0, 1, s.output_epoch, s.failure_error}; });
}
AstraEmuStatus frame(AstraEmuSession handle, AstraEmuFrameIdentity previous, AstraEmuFrameSurface *out, AstraEmuError *e) noexcept {
    if(out) *out = {};
    return boundary(e, [&] {
        auto &s = session(handle); require(out, "KRKR_FRAME_NULL"); std::lock_guard<std::mutex> lock(s.mutex);
        if(!s.latest) { out->state = AstraEmuFRAME_PENDING; return; }
        auto &slot = *s.latest; out->identity = slot.identity; out->info = slot.info; out->timestamp_ns = slot.timestamp;
        if(previous.session_epoch == slot.identity.session_epoch && previous.output_epoch == slot.identity.output_epoch && previous.generation == slot.identity.generation) {
            out->state = AstraEmuFRAME_UNCHANGED; return;
        }
        slot.pool->owners.fetch_add(1, std::memory_order_relaxed); slot.leases.fetch_add(1, std::memory_order_relaxed);
        out->state = AstraEmuFRAME_READY; out->cpu = {{slot.pixels.get(), slot.length}, &slot, release_frame};
    });
}
void cancel(AstraEmuSession handle) noexcept { if(handle) static_cast<Session *>(handle)->cancel(); }
AstraEmuStatus close(AstraEmuSession handle, AstraEmuError *e) noexcept {
    return boundary(e, [&] {
        std::unique_ptr<Session> s(&session(handle)); s->cancel();
        if(s->worker.joinable()) s->worker.join();
        occupied.store(false);
        if(s->failure != AstraEmuOK && s->failure != AstraEmuCANCELLED) {
            throw Failure(s->failure, s->failure_error);
        }
    });
}
AstraEmuStatus extension(AstraEmuBytes, uint32_t, const void **out) noexcept { if(out) *out = nullptr; return out ? AstraEmuUNSUPPORTED : AstraEmuINVALID_ARGUMENT; }
const AstraEmuCoreApi api{{sizeof(AstraEmuCoreApi), AstraEmuABI_VERSION}, initialize, describe, probe, open, input, control, state, frame, cancel, close, extension};
}
extern "C" __declspec(dllexport) AstraEmuStatus astra_emu_krkr_query(uint32_t version, const AstraEmuCoreApi **out) noexcept {
    if(!out) return AstraEmuINVALID_ARGUMENT;
    *out = version == AstraEmuABI_VERSION ? &api : nullptr;
    return *out ? AstraEmuOK : AstraEmuUNSUPPORTED;
}
extern "C" __declspec(dllexport) AstraEmuStatus astra_emu_query(uint32_t version, const AstraEmuCoreApi **out) noexcept {
    return astra_emu_krkr_query(version, out);
}
