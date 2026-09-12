// Hosted compatibility definitions for symbols whose upstream homes live in
// the excluded cocos2d shell (MainScene.cpp / CustomFileUtils.cpp). Each one
// preserves the platform-independent semantics the engine expects while the
// presentation-layer behavior stays unavailable in headless sessions.

#include "tjsCommHead.h"

#include <windows.h>

#include <filesystem>
#include <fstream>
#include <string>
#include <vector>

#include "EventIntf.h"
#include "Platform.h"
#include "Application.h"
#include "MenuItemIntf.h"
#include "RenderManager.h"
#include "LayerBitmapIntf.h"

// Defined in environ/win32/Platform.cpp under KRKR2_ASTRA_HOSTED.
std::string TVPGetDefaultFileDir();

// ---------------------------------------------------------------------------
// Console log (upstream: environ/cocos2d/MainScene.cpp)
// ---------------------------------------------------------------------------

void TVPConsoleLog(const ttstr &l, bool important) {
    (void)important;
    // Bounded routing to the process logger; the engine log carries game
    // text only when the game prints it, which matches the reference
    // console behavior.
    std::string text = l.AsNarrowStdString();
    OutputDebugStringA(text.c_str());
    OutputDebugStringA("\n");
}

// ---------------------------------------------------------------------------
// Patch-lib URL (upstream: environ/cocos2d/MainScene.cpp)
// ---------------------------------------------------------------------------

void TVPOpenPatchLibUrl(void) {
    // Opening a browser is a platform-shell behavior; hosted sessions have
    // no shell to open.
}

// ---------------------------------------------------------------------------
// Application home directory (upstream: cocos2d CustomFileUtils.cpp)
// ---------------------------------------------------------------------------

namespace {
std::filesystem::path to_path(const std::string &name) {
    return std::filesystem::path(name);
}
} // namespace

const std::vector<std::string> &TVPGetApplicationHomeDirectory(void) {
    static const std::vector<std::string> home = [] {
        std::vector<std::string> paths;
        paths.push_back(TVPGetDefaultFileDir());
        return paths;
    }();
    return home;
}

// ---------------------------------------------------------------------------
// Max texture size (upstream: GL renderer initialization in MainScene.cpp)
// ---------------------------------------------------------------------------

unsigned int TVPMaxTextureSize = 4096;

// ---------------------------------------------------------------------------
// OS/platform description strings (upstream: MainScene.cpp)
// ---------------------------------------------------------------------------

ttstr TVPGetOSName(void) { return ttstr("Astra Hosted (Windows)"); }
ttstr TVPGetPlatformName(void) { return ttstr("astra-hosted"); }

// ---------------------------------------------------------------------------
// Popup menu (upstream: MainScene.cpp) - no shell menus headless
// ---------------------------------------------------------------------------

void TVPShowPopMenu(tTJSNI_MenuItem *menu) { (void)menu; }

// ---------------------------------------------------------------------------
// TJS-namespaced console log overload (upstream: MainScene.cpp)
// ---------------------------------------------------------------------------

namespace TJS {
void TVPConsoleLog(const tTJSString &text) {
    std::string line = text.AsNarrowStdString();
    line.push_back(0x0A);
    OutputDebugStringA(line.c_str());
}
} // namespace TJS


// ---------------------------------------------------------------------------
// PVR texture codecs (upstream: visual/LoadPVRv3.cpp, visual/ogl/pvrtc.cpp)
// GPU-compressed texture paths; the software renderer never serves them.
// ---------------------------------------------------------------------------

#include <functional>
iTVPTexture2D *TVPLoadPVRv3(
    tTJSBinaryStream *src,
    const std::function<void(const ttstr &, const tTJSVariant &)> &metainfo) {
    (void)src;
    (void)metainfo;
    return nullptr;
}

void TVPSavePVRv3(void *formatdata, tTJSBinaryStream *dst,
                  const iTVPBaseBitmap *image, const ttstr &mode,
                  iTJSDispatch2 *meta) {
    (void)formatdata;
    (void)dst;
    (void)image;
    (void)mode;
    (void)meta;
}


// ---------------------------------------------------------------------------
// File utilities (upstream: cocos2d CustomFileUtils.cpp; Platform.cpp only
// provides TVPRenameFile)
// ---------------------------------------------------------------------------

bool TVPCopyFile(const std::string &from, const std::string &to) {
    std::error_code ec;
    std::filesystem::copy_file(to_path(from), to_path(to),
                               std::filesystem::copy_options::overwrite_existing,
                               ec);
    return !ec;
}

// ---------------------------------------------------------------------------
// Internal preference path (upstream: Platform.cpp, cocos writable path)
// ---------------------------------------------------------------------------

const std::string &TVPGetInternalPreferencePath() {
    static const std::string path = TVPGetDefaultFileDir();
    return path;
}

// ---------------------------------------------------------------------------
// File selector (upstream: cocos2d CustomFileUtils.cpp) - no shell headless
// ---------------------------------------------------------------------------

std::string TVPShowFileSelector(const std::string &title,
                                const std::string &initialDir,
                                std::string filter, bool mustExist) {
    (void)title;
    (void)initialDir;
    (void)filter;
    (void)mustExist;
    return std::string();
}

// ---------------------------------------------------------------------------
// Async input state (upstream: MainScene.cpp; DirectInput is not initialized
// in hosted sessions)
// ---------------------------------------------------------------------------

bool TVPGetJoyPadAsyncState(tjs_uint keycode, bool getcurrent) {
    (void)keycode;
    (void)getcurrent;
    return false;
}

bool TVPGetKeyMouseAsyncState(tjs_uint keycode, bool getcurrent) {
    (void)keycode;
    (void)getcurrent;
    return false;
}
