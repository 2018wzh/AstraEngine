#pragma once
#include <cstdint>

struct AstraKrkrModalText {
    const std::uint8_t *data;
    std::uint64_t bytes;
};
struct AstraKrkrModalRequest {
    AstraKrkrModalText caption;
    AstraKrkrModalText message;
    const AstraKrkrModalText *buttons;
    std::uint32_t button_count;
    std::uint32_t has_input;
    AstraKrkrModalText initial_text;
};
struct AstraKrkrModalResponse {
    std::uint32_t kind; // 0: cancelled, 1: submitted
    std::uint32_t button_index;
    std::uint8_t *text;
    std::uint64_t text_capacity;
    std::uint64_t text_bytes;
};
using AstraKrkrModalCallback = std::int32_t (*)(
    void *, const AstraKrkrModalRequest *, AstraKrkrModalResponse *);

static_assert(sizeof(void *) == 8, "The native adapter requires x64");
static_assert(sizeof(AstraKrkrModalText) == 16);
static_assert(sizeof(AstraKrkrModalRequest) == 64);
static_assert(sizeof(AstraKrkrModalResponse) == 32);
