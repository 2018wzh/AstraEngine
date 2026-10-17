#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include "astra_emu.h"
#include <atomic>
#include <cstdio>
#include <filesystem>
#include <stdexcept>
#include <cwchar>
#include <cstdlib>
#include <string>
#include <chrono>
#include <thread>
#include <string_view>
#include <cstring>

namespace {
std::atomic<uint64_t> cancelled{0};
std::atomic<bool> loop_entered{false};
bool reject_requests = false;
unsigned rejected_modals = 0;
uint32_t is_cancelled(void *, uint64_t epoch) { return cancelled.load() == epoch; }
void emit(void *, const AstraEmuLogRecord *record) {
    std::fprintf(stderr, "%.*s: %.*s\n", static_cast<int>(record->target.len), record->target.data,
        static_cast<int>(record->message.len), record->message.data);
    if(record->message.len && std::string_view(
        reinterpret_cast<const char *>(record->message.data), record->message.len)
        .find("KRKR_CANCEL_LOOP_ENTERED") != std::string_view::npos)
        loop_entered.store(true);
}
AstraEmuStatus window(void *, const AstraEmuWindowRequest *r, AstraEmuWindowResult *, AstraEmuError *error) {
    if(reject_requests) {
        if(r->kind == AstraEmuWINDOW_MODAL) ++rejected_modals;
        const char *code = r->kind == AstraEmuWINDOW_TITLE ? "TEST_TITLE_REJECTED" : "TEST_MODAL_REJECTED";
        const char *message = r->kind == AstraEmuWINDOW_TITLE ? "original title failure" : "secondary modal failure";
        *error = {};
        error->code_len = static_cast<uint32_t>(std::strlen(code));
        error->message_len = static_cast<uint32_t>(std::strlen(message));
        std::memcpy(error->code, code, error->code_len);
        std::memcpy(error->message, message, error->message_len);
        return AstraEmuUNSUPPORTED;
    }
    // Never dismiss a commercial game's modal automatically. Interactive game
    // acceptance uses Manager's actual typed modal service instead of this test.
    if(r->kind == AstraEmuWINDOW_TITLE) {
        if(r->text.len > 4096 || (r->text.len && !r->text.data)) return AstraEmuINVALID_ARGUMENT;
        const auto source = reinterpret_cast<const char *>(r->text.data);
        const int count = r->text.len ? MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, source,
            static_cast<int>(r->text.len), nullptr, 0) : 0;
        if(r->text.len && !count) return AstraEmuINVALID_ARGUMENT;
        std::wstring title(static_cast<size_t>(count), L'\0');
        if(count) MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, source, static_cast<int>(r->text.len), title.data(), count);
        return SetConsoleTitleW(title.c_str()) ? AstraEmuOK : AstraEmuUNSUPPORTED;
    }
    if(r->kind == AstraEmuWINDOW_MODAL) {
        std::fprintf(stderr, "Modal requires interactive host: %.*s\n", static_cast<int>(r->text.len), r->text.data);
        return AstraEmuUNSUPPORTED;
    }
    return AstraEmuUNSUPPORTED;
}
void invalidate(void *, uint64_t epoch, uint64_t) { cancelled.store(epoch); }
void check(AstraEmuStatus status, const AstraEmuError &e) {
    if(status != AstraEmuOK) {
        std::fprintf(stderr, "status=%d code=%.*s message=%.*s\n", status, e.code_len, e.code, e.message_len, e.message);
        throw std::runtime_error("native ABI call failed");
    }
}
uint64_t hash(AstraEmuBytes data) {
    uint64_t value = 1469598103934665603ull;
    for(size_t i = 0; i < data.len; ++i) value = (value ^ data.data[i]) * 1099511628211ull;
    return value;
}
void verify_public_canvas(const AstraEmuFrameSurface &frame) {
    if(frame.info.raster.width != 320 || frame.info.raster.height != 240 ||
       frame.info.logical.width != 320 || frame.info.logical.height != 240)
        throw std::runtime_error("public fixture canvas geometry changed");
    const auto pixel = [&](size_t x, size_t y) {
        return frame.cpu.pixels.data + y * frame.info.stride + x * 4;
    };
    const auto background = pixel(2, 2), rectangle = pixel(20, 20), alpha = pixel(140, 20);
    std::printf("canvas rgba bg=%u,%u,%u,%u rect=%u,%u,%u,%u alpha=%u,%u,%u,%u hash=%llu\n",
        background[0],background[1],background[2],background[3],
        rectangle[0],rectangle[1],rectangle[2],rectangle[3],
        alpha[0],alpha[1],alpha[2],alpha[3],static_cast<unsigned long long>(hash(frame.cpu.pixels)));
    if(background[0] != 32 || background[1] != 64 || background[2] != 96 ||
       rectangle[0] != 224 || rectangle[1] != 32 || rectangle[2] != 64 ||
       std::abs(int(alpha[0]) - 16) > 1 || std::abs(int(alpha[1]) - 160) > 1 ||
       std::abs(int(alpha[2]) - 48) > 1)
        throw std::runtime_error("original fill/PNG alpha pixels differ");
    size_t glyph_pixels = 0;
    for(size_t y = 88; y < 132; ++y) for(size_t x = 16; x < 220; ++x) {
        const auto p = pixel(x,y);
        glyph_pixels += p[0] != 32 || p[1] != 64 || p[2] != 96;
    }
    if(glyph_pixels < 20) throw std::runtime_error("original font glyph pixels missing");
    std::printf("glyph pixels=%zu\n", glyph_pixels);
}
}
int wmain(int argc, wchar_t **argv) {
    if(argc != 3 && !(argc == 4 && std::wcscmp(argv[3], L"--public-fixture") == 0)) {
        std::fprintf(stderr, "native_lifecycle <astra_emu_krkr.dll> <game copy> [--public-fixture]\n"); return 2;
    }
    try {
        // One module load, two sessions: no reload, respawn, secondary executable
        // or replacement renderer can satisfy this acceptance path.
        HMODULE module = LoadLibraryW(argv[1]);
        if(!module) throw std::runtime_error("LoadLibraryW failed");
        auto query = reinterpret_cast<AstraEmuQuery>(GetProcAddress(module, "astra_emu_query"));
        if(!query) throw std::runtime_error("query missing");
        const AstraEmuCoreApi *api = nullptr;
        AstraEmuError e{}; check(query(AstraEmuABI_VERSION, &api), e);
        AstraEmuDiagnostics diagnostic{{sizeof(diagnostic), AstraEmuABI_VERSION}, nullptr, emit, nullptr};
        check(api->initialize(&diagnostic, &e), e);
        const auto game = std::filesystem::path(argv[2]).u8string();
        if(argc == 4) {
            AstraEmuOpenRequest request{};
            request.header = {sizeof(request), AstraEmuABI_VERSION};
            request.game_path = {reinterpret_cast<const uint8_t *>(game.data()), game.size()};
            request.session_epoch = 5;
            request.window = {1280, 720, 1.0, 1, 0};
            request.host.header = {sizeof(request.host), AstraEmuABI_VERSION};
            request.host.cancellation = {nullptr, is_cancelled};
            request.host.window = {nullptr, window, invalidate};
            reject_requests = true;
            AstraEmuOpenResult failed{};
            const auto status = api->open(&request, &failed, &e);
            reject_requests = false;
            if(status != AstraEmuUNSUPPORTED || failed.session || rejected_modals != 1 ||
               std::string_view(reinterpret_cast<const char *>(e.code), e.code_len) != "TEST_TITLE_REJECTED" ||
               std::string_view(reinterpret_cast<const char *>(e.message), e.message_len) != "original title failure")
                throw std::runtime_error("secondary error modal replaced original title failure");
            std::puts("first typed host failure survives secondary error modal");
        }
        AstraEmuFrameSurface retained{};
        uint64_t retained_hash = 0;
        for(uint64_t epoch = 1; epoch <= 2; ++epoch) {
            AstraEmuOpenRequest request{};
            request.header = {sizeof(request), AstraEmuABI_VERSION};
            request.game_path = {reinterpret_cast<const uint8_t *>(game.data()), game.size()};
            request.session_epoch = epoch;
            request.window = {1280, 720, 1.0, 1, 0};
            request.host.header = {sizeof(request.host), AstraEmuABI_VERSION};
            request.host.cancellation = {nullptr, is_cancelled};
            request.host.window = {nullptr, window, invalidate};
            AstraEmuOpenResult opened{}; check(api->open(&request, &opened, &e), e);
            AstraEmuFrameSurface frame{}; check(api->frame(opened.session, {}, &frame, &e), e);
            if(frame.state != AstraEmuFRAME_READY || !frame.cpu.release || !frame.cpu.pixels.len)
                throw std::runtime_error("real native canvas missing");
            if(argc == 4) verify_public_canvas(frame);
            if(epoch == 1) { retained = frame; retained_hash = hash(frame.cpu.pixels); }
            else {
                if(hash(retained.cpu.pixels) != retained_hash) throw std::runtime_error("retained first-session lease changed");
                frame.cpu.release(frame.cpu.owner);
            }
            if(argc == 4) {
                auto displayed = frame.identity;
                std::this_thread::sleep_for(std::chrono::milliseconds(50));
                AstraEmuFrameSurface unchanged{};
                check(api->frame(opened.session, displayed, &unchanged, &e), e);
                if(unchanged.state != AstraEmuFRAME_UNCHANGED)
                    throw std::runtime_error("static original canvas did not remain Unchanged");
                for(unsigned change = 0; change < 4; ++change) {
                    AstraEmuInputEvent events[2]{};
                    events[0].kind = events[1].kind = AstraEmuINPUT_KEY;
                    events[0].physical_key = events[1].physical_key =
                        {reinterpret_cast<const uint8_t *>("KeyA"), 4};
                    events[0].pressed = 1;
                    check(api->input(opened.session, events, 2, &e), e);
                    AstraEmuFrameSurface updated{};
                    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(2);
                    do {
                        check(api->frame(opened.session, displayed, &updated, &e), e);
                        if(updated.state == AstraEmuFRAME_READY) break;
                        std::this_thread::sleep_for(std::chrono::milliseconds(2));
                    } while(std::chrono::steady_clock::now() < deadline);
                    if(updated.state != AstraEmuFRAME_READY || updated.identity.generation <= displayed.generation)
                        throw std::runtime_error("native key input did not change the canvas");
                    if(hash(updated.cpu.pixels) == retained_hash || hash(retained.cpu.pixels) != retained_hash)
                        throw std::runtime_error("changed native frame mutated the retained lease");
                    displayed = updated.identity;
                    updated.cpu.release(updated.cpu.owner);
                }
                std::printf("epoch=%llu static Unchanged and four input generations passed\n",
                    static_cast<unsigned long long>(epoch));
                if(epoch == 2) {
                    AstraEmuInputEvent exit{};
                    exit.kind = AstraEmuINPUT_KEY; exit.pressed = 1;
                    exit.physical_key = {reinterpret_cast<const uint8_t *>("Escape"), 6};
                    check(api->input(opened.session, &exit, 1, &e), e);
                    AstraEmuSessionState state{};
                    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(2);
                    do {
                        check(api->state(opened.session, &state, &e), e);
                        if(state.state == AstraEmuSESSION_FINISHED) break;
                        if(state.state == AstraEmuSESSION_FAILED) throw std::runtime_error("System.exit failed");
                        std::this_thread::sleep_for(std::chrono::milliseconds(2));
                    } while(std::chrono::steady_clock::now() < deadline);
                    if(state.state != AstraEmuSESSION_FINISHED) throw std::runtime_error("System.exit did not finish");
                    std::puts("native System.exit finished");
                }
            }
            cancelled.store(epoch);
            api->cancel(opened.session);
            check(api->close(opened.session, &e), e);
            if(hash(retained.cpu.pixels) != retained_hash) throw std::runtime_error("close invalidated old lease");
        }
        if(argc == 4) {
            const auto loop_game = (std::filesystem::u8path(game) / "cancel-loop").u8string();
            AstraEmuOpenRequest request{};
            request.header = {sizeof(request), AstraEmuABI_VERSION};
            request.game_path = {reinterpret_cast<const uint8_t *>(loop_game.data()), loop_game.size()};
            request.session_epoch = 3;
            request.window = {1280,720,1.0,1,0};
            request.host.header = {sizeof(request.host), AstraEmuABI_VERSION};
            request.host.cancellation = {nullptr,is_cancelled};
            request.host.window = {nullptr,window,invalidate};
            std::thread cancel_open([] {
                const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(5);
                while(!loop_entered.load() && std::chrono::steady_clock::now() < deadline)
                    std::this_thread::sleep_for(std::chrono::milliseconds(2));
                if(loop_entered.load()) std::this_thread::sleep_for(std::chrono::milliseconds(50));
                cancelled.store(3);
            });
            AstraEmuOpenResult aborted{};
            const auto status = api->open(&request, &aborted, &e);
            cancel_open.join();
            if(!loop_entered.load()) throw std::runtime_error("startup cancellation did not reach original TJS loop");
            if(status != AstraEmuCANCELLED || aborted.session)
                throw std::runtime_error("startup VM cancellation did not consume failed open");
            request.session_epoch = 4;
            request.game_path = {reinterpret_cast<const uint8_t *>(game.data()), game.size()};
            AstraEmuOpenResult reopened{};
            check(api->open(&request, &reopened, &e), e);
            AstraEmuFrameSurface frame{};
            check(api->frame(reopened.session, {}, &frame, &e), e);
            if(frame.state != AstraEmuFRAME_READY) throw std::runtime_error("reopen after cancelled startup has no canvas");
            verify_public_canvas(frame);
            frame.cpu.release(frame.cpu.owner);
            api->cancel(reopened.session);
            check(api->close(reopened.session, &e), e);
            if(hash(retained.cpu.pixels) != retained_hash) throw std::runtime_error("cancelled startup changed old lease");
            std::puts("startup cancellation and same-module reopen passed");
        }
        retained.cpu.release(retained.cpu.owner);
        // The module remains resident through process exit, as required for C
        // callback pointers. This test never unloads and reopens the VM library.
        std::puts("same-process open/close/reopen and cross-close immutable lease passed");
        return 0;
    } catch(const std::exception &e) { std::fprintf(stderr, "%s\n", e.what()); return 1; }
}
