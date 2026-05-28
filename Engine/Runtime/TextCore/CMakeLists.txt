astra_add_library(Astra_TextCore
    Private/TextCore.cpp
)

target_link_libraries(Astra_TextCore
    PUBLIC
        Astra_Core
    PRIVATE
        Freetype::Freetype
        harfbuzz::harfbuzz
)

target_compile_definitions(Astra_TextCore PRIVATE ASTRA_HAS_HARFBUZZ=1)
