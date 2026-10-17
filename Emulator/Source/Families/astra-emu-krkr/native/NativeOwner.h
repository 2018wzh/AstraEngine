#pragma once
#include "HostModalAbi.h"
#include <cstdint>
#include <exception>
#include <string>
struct NativeCancelled : std::exception {
    const char *what() const noexcept override { return "KRKR_CANCELLED"; }
};

// Private C++ session seam. Public callers use astra_emu.h exclusively.
struct AstraKrkrWindowState { std::uint32_t width, height, focused, visible; };
struct AstraKrkrFrameInfo { std::uint32_t width, height, logical_width, logical_height, stride; };
struct AstraKrkrCallbacks {
    void *context;
    std::uint32_t (*is_cancelled)(void *);
    AstraKrkrModalCallback modal;
    bool (*fullscreen)(void *, bool);
    void (*caption)(void *, const std::string &);
};
struct AstraKrkrBootOptions {
    const std::uint8_t *game_path_utf8;
    std::uint64_t game_path_bytes;
    const std::uint8_t *resource_root_utf8;
    std::uint64_t resource_root_bytes;
    const std::uint8_t *writable_root_utf8;
    std::uint64_t writable_root_bytes;
    AstraKrkrWindowState window;
    AstraKrkrCallbacks callbacks;
};
struct AstraKrkrEvent {
    std::uint32_t kind, key_code, key_state, modifiers, pointer_button, value;
    float x, y;
    std::uint32_t width, height;
    const std::uint8_t *text_utf8;
    std::uint64_t text_bytes;
};
struct AstraKrkrAdvanceResult {
    std::uint32_t state;
    std::int32_t exit_code;
    std::uint32_t reset_clock;
    std::uint32_t frame_changed;
};
