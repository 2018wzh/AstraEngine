// Astra hosted environ: drives the Kirikiri core without a platform UI.
//
// Replaces environ/cocos2d (AppDelegate/MainScene) for the AstraEMU family
// plugin: offscreen window layer, direct TVPPostInputEvent input, wall-clock
// engine ticks, hosted PCM push, and a C ABI for the Rust adapter.

#include "tjsCommHead.h"

#include <atomic>
#include <chrono>
#include <filesystem>
#include <condition_variable>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <spdlog/sinks/stdout_color_sinks.h>
#include <spdlog/spdlog.h>
#include <string>
#include <thread>
#include <vector>

#include "astra_krkr_host.h"

#include "EventIntf.h"
#include "SysInitIntf.h"
#include "SysInitImpl.h"
#include "StorageImpl.h"
#include "WindowImpl.h"
#include "TickCount.h"
#include "Random.h"
#include "Application.h"
#include "Platform.h"
#include "RenderManager.h"
#include "TVPWindow.h"
#include "vkdefine.h"

// Defined in environ/<platform>/Platform.cpp under KRKR2_ASTRA_HOSTED.
// Paths use UTF-16 code units: tjs_char is char16_t on every platform, while
// std::wstring is only 16-bit on Windows.
void TVPSetAstraHostedDirs(const std::u16string &game_dir,
                           const std::u16string &save_dir);

// ---------------------------------------------------------------------------
// Hosted state
// ---------------------------------------------------------------------------

namespace {

struct HostedState {
    std::mutex frame_mutex;
    std::vector<uint8_t> frame;  // RGBA, top-down
    tjs_uint frame_width = 0;
    tjs_uint frame_height = 0;

    std::atomic<bool> booted{false};
    std::atomic<bool> terminated{false};

    astra_krkr_host_callbacks callbacks{};

    std::u16string game_dir;
    std::u16string save_dir;
    std::u16string project_url;

    // Audio worker: the hosted renderer is pulled on a paced thread instead
    // of an OS audio callback.
    std::atomic<bool> audio_running{false};
    std::thread audio_thread;
};

HostedState &host() {
    static HostedState state;
    return state;
}

void hosted_log(uint8_t level, const char *message) {
    if(host().callbacks.log)
        host().callbacks.log(host().callbacks.user, level, message);
}

} // namespace

void astra_krkr_hosted_log(uint8_t level, const char *message) {
    hosted_log(level, message);
}

// ---------------------------------------------------------------------------
// Offscreen window layer
// ---------------------------------------------------------------------------

namespace {

class AstraHostedWindow : public iWindowLayer {
    tTJSNI_Window *Window;
    tjs_int LayerWidth = 0, LayerHeight = 0;
    tjs_int LastMouseX = 0, LastMouseY = 0;
    std::string Caption;
    bool Visible = false;

public:
    AstraHostedWindow *Prev = nullptr, *Next = nullptr;

    explicit AstraHostedWindow(tTJSNI_Window *window) : Window(window) {}

    tTJSNI_Window *GetWindow() const { return Window; }

    tjs_int GetLastMouseX() const { return LastMouseX; }
    tjs_int GetLastMouseY() const { return LastMouseY; }

    // --- input injection (mirrors the reference window's TVPPostInputEvent
    // calls, minus mouse-key emulation and touch gesture handling) ---

    void InjectKeyDown(tjs_uint key, tjs_uint32 shift) {
        TVPPushEnvironNoise(&key, sizeof(key));
        TVPPushEnvironNoise(&shift, sizeof(shift));
        if(Window)
            TVPPostInputEvent(
                new tTVPOnKeyDownInputEvent(Window, key, shift));
    }

    void InjectKeyUp(tjs_uint key, tjs_uint32 shift) {
        if(Window)
            TVPPostInputEvent(new tTVPOnKeyUpInputEvent(Window, key, shift));
    }

    void InjectKeyPress(tjs_uint32 utf32) {
        if(!Window || utf32 == 0)
            return;
        // TJS characters are 16-bit; fold astral codepoints into a
        // surrogate pair and deliver both halves as key presses.
        if(utf32 >= 0x10000 && utf32 <= 0x10FFFF) {
            const tjs_uint32 value = utf32 - 0x10000;
            const tjs_char high = tjs_char(0xD800 + (value >> 10));
            const tjs_char low = tjs_char(0xDC00 + (value & 0x3FF));
            TVPPostInputEvent(
                new tTVPOnKeyPressInputEvent(Window, high));
            TVPPostInputEvent(new tTVPOnKeyPressInputEvent(Window, low));
            return;
        }
        if(utf32 >= 0xD800 && utf32 < 0xE000)
            return;  // lone surrogate
        TVPPostInputEvent(
            new tTVPOnKeyPressInputEvent(Window, tjs_char(utf32)));
    }

    void InjectMouseMove(tjs_int x, tjs_int y, tjs_uint32 shift) {
        LastMouseX = x;
        LastMouseY = y;
        if(Window)
            TVPPostInputEvent(new tTVPOnMouseMoveInputEvent(
                                  Window, x, y, shift),
                              TVP_EPT_DISCARDABLE);
    }

    void InjectMouseDown(tjs_int x, tjs_int y, tTVPMouseButton button,
                         tjs_uint32 shift) {
        LastMouseX = x;
        LastMouseY = y;
        if(Window)
            TVPPostInputEvent(new tTVPOnMouseDownInputEvent(Window, x, y,
                                                            button, shift));
    }

    void InjectMouseUp(tjs_int x, tjs_int y, tTVPMouseButton button,
                       tjs_uint32 shift) {
        LastMouseX = x;
        LastMouseY = y;
        if(Window) {
            if(button == mbLeft)
                TVPPostInputEvent(
                    new tTVPOnClickInputEvent(Window, x, y));
            TVPPostInputEvent(
                new tTVPOnMouseUpInputEvent(Window, x, y, button, shift));
        }
    }

    void InjectMouseWheel(tjs_int delta, tjs_int x, tjs_int y,
                          tjs_uint32 shift) {
        if(Window)
            TVPPostInputEvent(new tTVPOnMouseWheelInputEvent(
                Window, shift, delta, x, y));
    }

    // --- iWindowLayer ---

    void SetPaintBoxSize(tjs_int w, tjs_int h) override {
        LayerWidth = w;
        LayerHeight = h;
    }

    bool GetFormEnabled() override { return true; }
    void SetDefaultMouseCursor() override {}
    void GetCursorPos(tjs_int &x, tjs_int &y) override {
        x = LastMouseX;
        y = LastMouseY;
    }
    void SetCursorPos(tjs_int x, tjs_int y) override {
        LastMouseX = x;
        LastMouseY = y;
    }
    void SetHintText(const ttstr &text) override {}
    void SetAttentionPoint(tjs_int left, tjs_int top,
                           const struct tTVPFont *font) override {}
    void ZoomRectangle(tjs_int &left, tjs_int &top, tjs_int &right,
                       tjs_int &bottom) override {}
    void BringToFront() override {}
    void ShowWindowAsModal() override {}
    bool GetVisible() override { return Visible; }
    void SetVisible(bool visible) override { Visible = visible; }
    const char *GetCaption() override { return Caption.c_str(); }
    void SetCaption(const std::string &caption) override {
        Caption = caption;
    }
    void SetWidth(tjs_int w) override { LayerWidth = w; }
    void SetHeight(tjs_int h) override { LayerHeight = h; }
    void SetSize(tjs_int w, tjs_int h) override {
        LayerWidth = w;
        LayerHeight = h;
    }
    void GetSize(tjs_int &w, tjs_int &h) override {
        w = LayerWidth;
        h = LayerHeight;
    }
    tjs_int GetWidth() const override { return LayerWidth; }
    tjs_int GetHeight() const override { return LayerHeight; }
    void GetWinSize(tjs_int &w, tjs_int &h) override {
        w = LayerWidth;
        h = LayerHeight;
    }
    void SetZoom(tjs_int numer, tjs_int denom) override {}

    void UpdateDrawBuffer(iTVPTexture2D *tex) override {
        if(!tex)
            return;
        if(!tex)
            return;
        const tjs_uint width = tex->GetWidth();
        const tjs_uint height = tex->GetHeight();
        if(width == 0 || height == 0)
            return;
        const void *pixels = tex->GetPixelData();
        if(!pixels)
            return;
        const tjs_int pitch = tex->GetPitch();
        std::lock_guard<std::mutex> lock(host().frame_mutex);
        host().frame_width = width;
        host().frame_height = height;
        host().frame.resize(size_t(width) * height * 4);
        for(tjs_uint row = 0; row < height; ++row) {
            const uint8_t *src =
                static_cast<const uint8_t *>(pixels) + size_t(pitch) * row;
            uint8_t *dst = host().frame.data() + size_t(width) * 4 * row;
            std::memcpy(dst, src, size_t(width) * 4);
        }
    }

    void InvalidateClose() override {}
    bool GetWindowActive() override { return true; }

    void Close() override {
        // Closing through the "close" method: forward to the core window so
        // onCloseQuery/onClose fire, then hide.
        if(Window) {
            Window->Close();
        }
        Visible = false;
    }

    void OnCloseQueryCalled(bool allowed) override {
        if(allowed)
            Visible = false;
    }

    void InternalKeyDown(tjs_uint16 key, tjs_uint32 shift) override {
        InjectKeyDown(key, shift);
    }
    void OnKeyUp(tjs_uint16 vk, int shift) override {
        InjectKeyUp(vk, static_cast<tjs_uint32>(shift));
    }
    void OnKeyPress(tjs_uint16 vk, int repeat, bool prevkeystate,
                    bool convertkey) override {
        InjectKeyPress(vk);
    }
    tTVPImeMode GetDefaultImeMode() const override { return imDisable; }
    void SetImeMode(tTVPImeMode mode) override {}
    void ResetImeMode() override {}
    void UpdateWindow(tTVPUpdateType type) override {}
    void SetVisibleFromScript(bool visible) override { Visible = visible; }
    void SetUseMouseKey(bool enabled) override {}
    bool GetUseMouseKey() const override { return false; }
    void ResetMouseVelocity() override {}
    void ResetTouchVelocity(tjs_int id) override {}
    bool GetMouseVelocity(float &x, float &y, float &speed) const override {
        x = y = speed = 0;
        return false;
    }
    void TickBeat() override {}
    cocos2d::Node *GetPrimaryArea() override { return nullptr; }
};

AstraHostedWindow *g_windows = nullptr;  // top of the doubly-linked list

AstraHostedWindow *active_window() {
    // The most recently created visible window mirrors the reference
    // implementation's current-layer tracking.
    for(AstraHostedWindow *window = g_windows; window;
        window = window->Prev) {
        if(window->GetVisible())
            return window;
    }
    return g_windows;
}

} // namespace

// Core factory hooks (the reference implementation defines these in
// environ/cocos2d/MainScene.cpp; the hosted build links these instead).
iWindowLayer *TVPCreateAndAddWindow(tTJSNI_Window *w) {
    auto *window = new AstraHostedWindow(w);
    window->Prev = g_windows;
    if(g_windows)
        g_windows->Next = window;
    g_windows = window;
    return window;
}

void TVPRemoveWindowLayer(iWindowLayer *lay) {
    delete static_cast<AstraHostedWindow *>(lay);
}

tTJSNI_Window *TVPGetActiveWindow() {
    AstraHostedWindow *window = active_window();
    return window ? window->GetWindow() : nullptr;
}

// ---------------------------------------------------------------------------
// Audio worker
// ---------------------------------------------------------------------------

namespace {

// Pulls mixed PCM from the hosted WaveMixer renderer (WaveMixer.cpp patched
// to expose it) and pushes it to the family audio bridge.
extern "C" void astra_krkr_hosted_fill_audio(int16_t *samples,
                                             uint32_t frame_count);

void audio_worker_loop() {
    constexpr uint32_t kFramesPerChunk = 480;  // 10ms @ 48kHz
    constexpr auto kChunkDuration = std::chrono::microseconds(10'000);
    std::vector<int16_t> buffer(kFramesPerChunk * ASTRA_KRKR_CHANNELS);
    auto next_deadline = std::chrono::steady_clock::now();
    while(host().audio_running.load(std::memory_order_acquire)) {
        next_deadline += kChunkDuration;
        astra_krkr_hosted_fill_audio(buffer.data(), kFramesPerChunk);
        if(host().callbacks.push_pcm) {
            host().callbacks.push_pcm(host().callbacks.user, buffer.data(),
                                      kFramesPerChunk);
        }
        std::this_thread::sleep_until(next_deadline);
    }
}

} // namespace

void astra_krkr_hosted_push_pcm(const int16_t *samples, uint32_t frame_count) {
    if(host().callbacks.push_pcm)
        host().callbacks.push_pcm(host().callbacks.user, samples,
                                  frame_count);
}

// ---------------------------------------------------------------------------
// C ABI
// ---------------------------------------------------------------------------

// tjs_char is char16_t; paths arrive as UTF-16 code units on every platform,
// so the conversion to ttstr is a straight element copy.
static ttstr wide_to_ttstr(const std::u16string &wide) {
    std::basic_string<tjs_char> out(wide.begin(), wide.end());
    return ttstr(out);
}

extern "C" uint32_t astra_krkr_abi_version(void) {
    return ASTRA_KRKR_HOST_ABI_VERSION;
}

static std::u16string utf8_to_wide(const char *text) {
    if(!text)
        return std::u16string();
    std::u16string out;
    out.reserve(std::strlen(text));
    for(size_t i = 0; text[i];) {
        const unsigned char c = static_cast<unsigned char>(text[i++]);
        uint32_t cp = 0;
        int extra = 0;
        if(c < 0x80) {
            cp = c;
        } else if((c & 0xE0) == 0xC0) {
            cp = c & 0x1F;
            extra = 1;
        } else if((c & 0xF0) == 0xE0) {
            cp = c & 0x0F;
            extra = 2;
        } else if((c & 0xF8) == 0xF0) {
            cp = c & 0x07;
            extra = 3;
        } else {
            continue;  // invalid lead byte
        }
        bool valid = true;
        for(int k = 0; k < extra; ++k) {
            const unsigned char cc = static_cast<unsigned char>(text[i]);
            if((cc & 0xC0) != 0x80) {
                valid = false;
                break;
            }
            ++i;
            cp = (cp << 6) | (cc & 0x3F);
        }
        if(!valid)
            break;
        if(cp >= 0xD800 && cp < 0xE000)
            continue;  // surrogate: not produced by valid UTF-8
        if(cp >= 0x10000) {
            cp -= 0x10000;
            out.push_back(static_cast<char16_t>(0xD800 + (cp >> 10)));
            out.push_back(static_cast<char16_t>(0xDC00 + (cp & 0x3FF)));
        } else {
            out.push_back(static_cast<char16_t>(cp));
        }
    }
    return out;
}

extern "C" int32_t astra_krkr_boot(const astra_krkr_boot_config *config) {
    if(!config || config->abi != ASTRA_KRKR_HOST_ABI_VERSION ||
       !config->game_dir || !config->save_dir)
        return astra_krkr_err_arg;
    if(host().booted.load(std::memory_order_acquire))
        return astra_krkr_err_state;

    // Platform shells register these named loggers in main(); several call
    // sites (the TJS exception path, the motionplayer LOGGER macro) assume
    // they exist and would dereference null.
    for(const char *name : {"core", "tjs2", "plugin"}) {
        if(!spdlog::get(name))
            spdlog::stdout_color_mt(name);
    }

    HostedState &state = host();
    state.callbacks = config->callbacks;
    state.game_dir = utf8_to_wide(config->game_dir);
    state.save_dir = utf8_to_wide(config->save_dir);
    state.terminated.store(false, std::memory_order_release);
    {
        std::lock_guard<std::mutex> lock(state.frame_mutex);
        state.frame.clear();
        state.frame_width = 0;
        state.frame_height = 0;
    }
    TVPSetAstraHostedDirs(state.game_dir, state.save_dir);

    // StartApplication assigns TVPProjectDir only after the config managers
    // read their XML, and those reads resolve through TVPGetAppPath (a
    // one-shot static). Seed the project directory before anything can
    // observe an empty one.
    TVPNativeProjectDir = wide_to_ttstr(state.game_dir);
    {
        // The project unit for a Kirikiri game is data.xp3 (or the loose
        // startup.tjs directory). Follow the reference shells: pass the
        // archive file so the engine mounts it as the project root. The
        // base normalizer treats "E:" as a relative directory, so build
        // the storage URL directly.
        std::u16string game = state.game_dir;
        for(char16_t &c : game) {
            if(c == u'\\')
                c = u'/';
        }
        std::u16string lower = game;
        for(char16_t &c : lower) {
            if(c >= u'A' && c <= u'Z')
                c = c + (u'a' - u'A');
        }
        std::u16string project;
        if(lower.size() >= 4 && lower.substr(lower.size() - 4) == u".xp3") {
            project = game;
        } else {
            std::u16string candidate = game;
            if(!candidate.empty() && candidate.back() != u'/')
                candidate += u'/';
            candidate += u"data.xp3";
            std::error_code ec;
            if(std::filesystem::exists(
                   std::filesystem::path(candidate.begin(), candidate.end()),
                   ec)) {
                project = candidate;
            } else {
                project = game;
            }
        }
        std::u16string url = u"file://" + project;
        state.project_url = url;
        TVPNativeProjectDir = wide_to_ttstr(project);
        TVPProjectDir = wide_to_ttstr(url);
    }

    hosted_log(2, "engine boot begin");
    try {
        Application = new tTVPApplication();
        if(!Application->StartApplication(wide_to_ttstr(state.project_url))) {
            hosted_log(4, "engine boot: StartApplication failed");
            delete Application;
            Application = nullptr;
            return astra_krkr_err_boot;
        }
    } catch(...) {
        hosted_log(4, "engine boot: exception");
        return astra_krkr_err_boot;
    }

    state.audio_running.store(true, std::memory_order_release);
    state.audio_thread = std::thread(audio_worker_loop);
    state.booted.store(true, std::memory_order_release);
    hosted_log(2, "engine boot complete");
    return astra_krkr_ok;
}

static tjs_uint32 hosted_shift_state(const astra_krkr_input_event &event) {
    tjs_uint32 shift = 0;
    if(event.shift)
        shift |= TVP_SS_SHIFT;
    if(event.control)
        shift |= TVP_SS_CTRL;
    return shift;
}

static tTVPMouseButton hosted_mouse_button(uint8_t button) {
    switch(button) {
    case 1:
        return mbRight;
    case 2:
        return mbMiddle;
    case 0:
    default:
        return mbLeft;
    }
}

extern "C" int32_t astra_krkr_tick(double elapsed_seconds,
                                   const astra_krkr_input_event *events,
                                   uint32_t event_count) {
    (void)elapsed_seconds;  // the engine owns a wall-clock tick policy
    if(!host().booted.load(std::memory_order_acquire))
        return astra_krkr_err_state;

    if(AstraHostedWindow *window = active_window()) {
        for(uint32_t index = 0; index < event_count; ++index) {
            const astra_krkr_input_event &event = events[index];
            const tjs_uint32 shift = hosted_shift_state(event);
            switch(event.kind) {
            case astra_krkr_input_key_down:
                window->InjectKeyDown(event.key, shift);
                break;
            case astra_krkr_input_key_up:
                window->InjectKeyUp(event.key, shift);
                break;
            case astra_krkr_input_text:
                window->InjectKeyPress(event.utf32);
                break;
            case astra_krkr_input_mouse_move:
                window->InjectMouseMove(event.x, event.y, shift);
                break;
            case astra_krkr_input_mouse_down:
                window->InjectMouseDown(event.x, event.y,
                                        hosted_mouse_button(event.button),
                                        shift);
                break;
            case astra_krkr_input_mouse_up:
                window->InjectMouseUp(event.x, event.y,
                                      hosted_mouse_button(event.button),
                                      shift);
                break;
            case astra_krkr_input_wheel:
                window->InjectMouseWheel(event.wheel * -120, event.x, event.y,
                                         shift);
                break;
            default:
                break;
            }
        }
    }

    try {
        Application->Run();
    } catch(...) {
        hosted_log(4, "engine tick: exception");
        host().terminated.store(true, std::memory_order_release);
        return astra_krkr_err_terminated;
    }

    if(TVPTerminated)
        host().terminated.store(true, std::memory_order_release);

    if(host().terminated.load(std::memory_order_acquire))
        return astra_krkr_err_terminated;
    return astra_krkr_ok;
}

extern "C" int32_t astra_krkr_copy_frame(uint8_t *dst, uint32_t capacity,
                                         uint32_t *width, uint32_t *height) {
    if(!width || !height)
        return astra_krkr_err_arg;
    HostedState &state = host();
    std::lock_guard<std::mutex> lock(state.frame_mutex);
    *width = state.frame_width;
    *height = state.frame_height;
    if(state.frame_width == 0 || state.frame_height == 0)
        return astra_krkr_err_state;
    const uint32_t required = state.frame_width * state.frame_height * 4;
    if(!dst || capacity < required)
        return astra_krkr_err_arg;
    std::memcpy(dst, state.frame.data(), required);
    return astra_krkr_ok;
}

extern "C" int32_t astra_krkr_audio_format(uint32_t *sample_rate,
                                           uint16_t *channels) {
    if(!sample_rate || !channels)
        return astra_krkr_err_arg;
    *sample_rate = ASTRA_KRKR_SAMPLE_RATE;
    *channels = ASTRA_KRKR_CHANNELS;
    return astra_krkr_ok;
}

extern "C" int32_t astra_krkr_terminated(uint8_t *terminated) {
    if(!terminated)
        return astra_krkr_err_arg;
    *terminated =
        host().terminated.load(std::memory_order_acquire) ? 1 : 0;
    return astra_krkr_ok;
}

extern "C" int32_t astra_krkr_shutdown(void) {
    if(!host().booted.load(std::memory_order_acquire))
        return astra_krkr_err_state;
    HostedState &state = host();

    // Terminate asynchronously, then pump once so Run() performs the system
    // uninitialization on the session thread; TVPExitApplication is patched
    // to only flag under KRKR2_ASTRA_HOSTED, so this returns.
    try {
        TVPTerminateAsync(0);
        Application->Run();
    } catch(...) {
        hosted_log(3, "engine shutdown: exception during terminate pump");
    }
    delete Application;
    Application = nullptr;

    state.audio_running.store(false, std::memory_order_release);
    if(state.audio_thread.joinable())
        state.audio_thread.join();

    state.booted.store(false, std::memory_order_release);
    state.terminated.store(false, std::memory_order_release);
    hosted_log(2, "engine shutdown complete");
    return astra_krkr_ok;
}
