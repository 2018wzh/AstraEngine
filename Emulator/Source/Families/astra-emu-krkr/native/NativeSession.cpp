#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <winsock2.h>
#include <windows.h>
#include "NativeSession.h"
#include "HostModal.h"
#include "environ/cocos2d/AppDelegate.h"
#include "environ/cocos2d/MainScene.h"
#include "environ/ui/GlobalPreferenceForm.h"
#include "environ/ui/extension/UIExtension.h"
#include "environ/Application.h"
#include "environ/ConfigManager/GlobalConfigManager.h"
#include "base/SysInitIntf.h"
#include "movie/ffmpeg/krffmpeg.h"
#include "visual/RenderManager.h"
#include "visual/impl/WindowImpl.h"
#include "platform/CCApplication.h"
#include "platform/CCFileUtils.h"
#include "platform/desktop/CCGLViewImpl-desktop.h"
#include "base/CCDirector.h"
#include "base/CCAutoreleasePool.h"
#include "base/CCScheduler.h"
extern void TVPReleaseAudioRenderer();
#include "tjs2/tjsExecutionControl.h"
#include <GLFW/glfw3.h>
#include <spdlog/spdlog.h>
#include <algorithm>
#include <chrono>
#include <cstring>
#include <exception>
#include <filesystem>
#include <stdexcept>

namespace astra::krkr {
class EmbeddedView final : public cocos2d::GLViewImpl {
public:
    static EmbeddedView *create() {
        auto view = std::unique_ptr<EmbeddedView>(new EmbeddedView());
        glfwWindowHint(GLFW_VISIBLE, GLFW_FALSE);
        glfwWindowHint(GLFW_FOCUSED, GLFW_FALSE);
        if(!view->initWithRect("Astra KrKr context", cocos2d::Rect(0, 0, 960, 640), 1.0f, false))
            throw std::runtime_error("ASTRA_KRKR_NATIVE_GL_CONTEXT");
        view->autorelease();
        return view.release();
    }
    ~EmbeddedView() override = default;
    void pollEvents() override {
        // Director calls this virtual endpoint while drawing. Physical events
        // enter exclusively through CoreApi.input; no GLFW event pump runs here.
    }
    void swapBuffers() override {
        // Rendering still uses the original GL context; only Manager presents.
        glFlush();
    }
};
static std::string path_text(const std::uint8_t *data, std::uint64_t bytes) {
    if(!data || !bytes || bytes > 4096 || std::memchr(data, 0, bytes))
        throw std::runtime_error("ASTRA_KRKR_NATIVE_PATH_BOUND");
    const auto text = reinterpret_cast<const char *>(data);
    if(!MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, text, static_cast<int>(bytes), nullptr, 0))
        throw std::runtime_error("ASTRA_KRKR_NATIVE_PATH_UTF8");
    std::string result(text, bytes);
    const auto path = std::filesystem::u8path(result);
    if(!path.is_absolute() || !std::filesystem::is_directory(path))
        throw std::runtime_error("ASTRA_KRKR_NATIVE_PATH_DIRECTORY");
    std::replace(result.begin(), result.end(), '\\', '/');
    if(result.back() != '/') result += '/';
    return result;
}
NativeSession::NativeSession(const AstraKrkrBootOptions &options)
    : callbacks_(options.callbacks),
      host_{this, "opengl", changed, ready, exit, fullscreen, caption} {}
NativeSession::~NativeSession() {
    if(!closed_) {
        try { shutdown(); }
        catch(...) { spdlog::error("ASTRA_KRKR_NATIVE_DESTRUCTOR_CLEANUP_FAILED"); }
    }
}
void NativeSession::check_thread() const {
    if(std::this_thread::get_id() != thread_)
        throw std::runtime_error("ASTRA_KRKR_NATIVE_OWNER_THREAD");
}
void NativeSession::changed(void *context, tTJSNI_BaseWindow *window) {
    auto &self = *static_cast<NativeSession *>(context);
    if(!self.window_ || self.window_ == window) self.dirty_ = true;
}
void NativeSession::ready(void *context, tTJSNI_BaseWindow *window, iTVPTexture2D *texture,
                          unsigned int width, unsigned int height) {
    auto &self = *static_cast<NativeSession *>(context);
    // Follow the original active game window, not the Cocos selector or UI.
    if(auto active = TVPGetCurrentWindowNative(); active && window != active) return;
    if(self.texture_ != texture) {
        texture->AddRef();
        if(self.texture_) self.texture_->Release();
        self.texture_ = texture;
        self.dirty_ = true;
    }
    if(self.window_ != window || self.logical_width_ != width || self.logical_height_ != height)
        self.dirty_ = true;
    self.window_ = window;
    self.logical_width_ = width;
    self.logical_height_ = height;
    self.changed_ |= self.dirty_;
    self.dirty_ = false;
}
void NativeSession::exit(void *context, int code) {
    auto &self = *static_cast<NativeSession *>(context);
    self.exited_ = true;
    self.exit_code_ = code;
}
bool NativeSession::fullscreen(void *context, iWindowLayer *, bool enabled) {
    auto &self = *static_cast<NativeSession *>(context);
    return self.callbacks_.fullscreen(self.callbacks_.context, enabled);
}
void NativeSession::caption(void *context, iWindowLayer *, const std::string &text) {
    if(text.size() > 4096 || std::any_of(text.begin(), text.end(), [](unsigned char c) { return c < 32 || c == 127; }))
        throw std::runtime_error("ASTRA_KRKR_NATIVE_CAPTION_BOUND");
    auto &self = *static_cast<NativeSession *>(context);
    self.callbacks_.caption(self.callbacks_.context, text);
}
void NativeSession::tick(float seconds) {
    if(callbacks_.is_cancelled(callbacks_.context))
        throw NativeCancelled();
    auto director = cocos2d::Director::getInstance();
    director->mainLoop(seconds);
}
void NativeSession::boot(const AstraKrkrBootOptions &options) {
    check_thread();
    if(!options.window.width || !options.window.height || options.window.focused > 1 || options.window.visible > 1)
        throw std::runtime_error("ASTRA_KRKR_NATIVE_WINDOW_STATE");
    auto game = path_text(options.game_path_utf8, options.game_path_bytes);
    if(!std::filesystem::is_regular_file(std::filesystem::u8path(game + "startup.tjs"))) {
        if(!std::filesystem::is_regular_file(std::filesystem::u8path(game + "data.xp3")))
            throw std::runtime_error("ASTRA_KRKR_NATIVE_GAME_ENTRY");
        game += "data.xp3"; // Same entry chosen by the original file selector.
    }
    const auto resources = path_text(options.resource_root_utf8, options.resource_root_bytes);
    const auto writable = path_text(options.writable_root_utf8, options.writable_root_bytes);
    if(!TVPInstallEmbeddedSessionHost(&host_))
        throw std::runtime_error("ASTRA_KRKR_NATIVE_SESSION_IN_USE");
    installed_ = true;
    modal_ = std::make_unique<HostModal>(callbacks_.context, callbacks_.modal);
    TVPBeginSystemSession();
    TJS::TJSExecutionCancellationContext = this;
    TJS::TJSExecutionCancelled = [](void *context) {
        const auto self = static_cast<NativeSession *>(context);
        return self->exited_ || self->callbacks_.is_cancelled(self->callbacks_.context) != 0;
    };
    if(!::Application) ::Application = new tTVPApplication();
    cocos2d::FileUtils::getInstance()->setDefaultResourceRootPath(resources);
    cocos2d::FileUtils::getInstance()->setWritablePath(writable);
    TVPInitLibAVCodec();
    GlobalConfigManager::GetInstance()->Reload();
    app_ = std::make_unique<TVPAppDelegate>();
    auto application = static_cast<cocos2d::ApplicationProtocol *>(app_.get());
    application->initGLContextAttrs();
    launched_ = true; // Includes partial launch cleanup.
    cocos2d::Director::getInstance()->setOpenGLView(EmbeddedView::create());
    if(!application->applicationDidFinishLaunching())
        throw std::runtime_error("ASTRA_KRKR_NATIVE_APPLICATION_LAUNCH");
    auto director = cocos2d::Director::getInstance();
    director->setDisplayStats(false);
    auto view = director->getOpenGLView();
    if(!view || !view->getWin32Window()) throw std::runtime_error("ASTRA_KRKR_NATIVE_GL_WINDOW");
    ShowWindow(view->getWin32Window(), SW_HIDE);
    // The context is created invisible; its present endpoint only flushes GL.
    glfwSwapInterval(0);
    if(options.window.focused) ::Application->OnActivate();
    else ::Application->OnDeactivate();
    auto scene = TVPMainScene::GetInstance();
    scene->unschedule("launch");
    TVPGlobalPreferenceForm::Initialize();
    if(!scene->startupFrom(game)) throw std::runtime_error("ASTRA_KRKR_NATIVE_GAME_START");
    const auto started = std::chrono::steady_clock::now();
    const auto initial_wait = modal_->owner_waiting();
    auto previous = started;
    auto previous_wait = initial_wait;
    while(!texture_ && !exited_) {
        const auto now = std::chrono::steady_clock::now();
        const auto waiting = modal_->owner_waiting();
        if(now - started - (waiting - initial_wait) >= std::chrono::seconds(5))
            throw std::runtime_error("ASTRA_KRKR_NATIVE_GAME_CANVAS_TIMEOUT");
        tick(std::chrono::duration<float>(now - previous - (waiting - previous_wait)).count());
        previous = now;
        previous_wait = waiting;
        if(!texture_ && !exited_) std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }
    if(exited_ || !texture_) throw std::runtime_error("ASTRA_KRKR_NATIVE_EXIT_BEFORE_CANVAS");
    if(TVPGetRenderManager()->IsSoftware()) throw std::runtime_error("ASTRA_KRKR_NATIVE_GPU_REQUIRED");
    const auto info = frame_info();
    spdlog::info("ASTRA_KRKR_NATIVE_READY raster={}x{} stage={}x{} stride={} focused={} renderer={}",
        info.width, info.height, info.logical_width, info.logical_height, info.stride,
        ::Application->GetActivating(), reinterpret_cast<const char *>(glGetString(GL_RENDERER)));
}
AstraKrkrFrameInfo NativeSession::frame_info() const {
    check_thread();
    if(!texture_ || texture_->GetFormat() != TVPTextureFormat::RGBA)
        throw std::runtime_error("ASTRA_KRKR_NATIVE_FRAME_FORMAT");
    const auto pitch = texture_->GetPitch();
    AstraKrkrFrameInfo result{texture_->GetWidth(), texture_->GetHeight(), logical_width_, logical_height_,
        pitch > 0 ? static_cast<std::uint32_t>(pitch) : 0};
    if(!result.width || !result.height || !result.logical_width || !result.logical_height ||
       static_cast<std::uint64_t>(result.width) * 4 > result.stride ||
       static_cast<std::uint64_t>(result.stride) * result.height > 67108864 ||
       static_cast<std::uint64_t>(result.width) * result.logical_height !=
       static_cast<std::uint64_t>(result.height) * result.logical_width)
        throw std::runtime_error("ASTRA_KRKR_NATIVE_FRAME_GEOMETRY");
    return result;
}
void NativeSession::copy_frame(const AstraKrkrFrameInfo &expected, std::uint8_t *target, std::uint64_t bytes) {
    const auto actual = frame_info();
    if(std::memcmp(&expected, &actual, sizeof(actual)) || !target ||
       bytes != static_cast<std::uint64_t>(actual.stride) * actual.height)
        throw std::runtime_error("ASTRA_KRKR_NATIVE_FRAME_TARGET");
    const auto source = static_cast<const std::uint8_t *>(texture_->GetPixelData());
    if(!source || texture_->GetPitch() != actual.stride)
        throw std::runtime_error("ASTRA_KRKR_NATIVE_FRAME_READBACK");
    // Do not copy uninitialized or off-canvas native padding into a CPU lease.
    // Preserve the original stride, writing visible bytes once and clearing padding.
    const auto row_bytes = static_cast<std::size_t>(actual.width) * 4;
    for(unsigned int row = 0; row < actual.height; ++row) {
        const auto offset = static_cast<std::size_t>(row) * actual.stride;
        std::memcpy(target + offset, source + offset, row_bytes);
        std::memset(target + offset + row_bytes, 0, actual.stride - row_bytes);
    }
    changed_ = false;
}
AstraKrkrAdvanceResult NativeSession::advance(std::uint64_t elapsed, const AstraKrkrEvent *events, std::uint32_t count) {
    check_thread();
    if(closed_ || (count && !events) || count > 4096)
        throw std::runtime_error("ASTRA_KRKR_NATIVE_ADVANCE_ARGUMENT");
    if(!exited_) {
        for(std::uint32_t i = 0; i < count; ++i) input_.apply(events[i]);
        if(!input_.suspended()) tick(static_cast<float>(static_cast<double>(elapsed) / 1000000000.0));
    }
    AstraKrkrAdvanceResult result{};
    result.state = exited_ ? 2 : 0;
    result.exit_code = exit_code_;
    result.reset_clock = input_.take_reset_clock();
    result.frame_changed = changed_ && !exited_;
    return result;
}
void NativeSession::shutdown() {
    check_thread();
    if(closed_) return;
    closed_ = true;
    std::exception_ptr failure;
    const auto cleanup = [&](auto &&call) {
        try { call(); } catch(...) { if(!failure) failure = std::current_exception(); }
    };
    if(texture_) { cleanup([&] { texture_->Release(); }); texture_ = nullptr; }
    if(installed_ && !TVPSystemUninitCalled) cleanup([] { TVPSystemUninit(); });
    if(installed_) cleanup([] { delete Application; Application = nullptr; });
    cleanup([] { TVPReleaseAudioRenderer(); });
    if(launched_) {
        auto director = cocos2d::Director::getInstance();
        cleanup([&] { director->getScheduler()->unscheduleAll(); });
        cleanup([&] {
            if(auto scene = director->getRunningScene()) scene->removeAllChildrenWithCleanup(true);
        });
        cleanup([] { TVPUninitUIExtension(); });
        cleanup([] { cocos2d::PoolManager::getInstance()->getCurrentPool()->clear(); });
        cleanup([] { iTVPTexture2D::RecycleProcess(); });
        cleanup([] { TVPReleaseRenderManagers(); });
    }
    if(launched_) cleanup([] {
        auto director = cocos2d::Director::getInstance();
        director->end();
        director->mainLoop(0);
    });
    app_.reset();
    modal_.reset();
    TJS::TJSExecutionCancelled = nullptr;
    TJS::TJSExecutionCancellationContext = nullptr;
    if(installed_) { TVPRemoveEmbeddedSessionHost(&host_); installed_ = false; }
    spdlog::info("ASTRA_KRKR_NATIVE_CLOSED cleanup_failed={}", bool(failure));
    if(failure) std::rethrow_exception(failure);
}
}

std::uint64_t astra::krkr::NativeSession::frame_interval_ns() const {
    return static_cast<std::uint64_t>(cocos2d::Director::getInstance()->getAnimationInterval() * 1000000000.0);
}
std::uint64_t astra::krkr::NativeSession::modal_wait_ns() const {
    return static_cast<std::uint64_t>(std::chrono::duration_cast<std::chrono::nanoseconds>(modal_->owner_waiting()).count());
}
