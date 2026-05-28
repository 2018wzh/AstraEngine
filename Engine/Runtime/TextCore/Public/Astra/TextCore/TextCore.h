#pragma once

#include <string>

namespace astra {

struct TextSystemInfo {
    bool freetype_linked = false;
    bool harfbuzz_linked = false;
    std::string description;
};

TextSystemInfo query_text_system();

} // namespace astra
