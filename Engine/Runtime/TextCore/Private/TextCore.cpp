#include <Astra/TextCore/TextCore.h>

#include <ft2build.h>
#include FT_FREETYPE_H

#if defined(ASTRA_HAS_HARFBUZZ)
#include <hb.h>
#endif

namespace astra {

TextSystemInfo query_text_system() {
    TextSystemInfo info;
    info.freetype_linked = true;
    info.harfbuzz_linked =
#if defined(ASTRA_HAS_HARFBUZZ)
        true;
#else
        false;
#endif
    info.description =
        info.harfbuzz_linked
            ? "FreeType + HarfBuzz linked; first renderer uses SDL debug text."
            : "FreeType linked; HarfBuzz is optional until vcpkg meson is available.";
    return info;
}

} // namespace astra
