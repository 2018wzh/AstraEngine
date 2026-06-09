set(ASTRA_MEDIA_SOURCES
    Private/Media.cpp
    Private/SdlRendererStub.cpp
)

astra_add_library(Astra_Media ${ASTRA_MEDIA_SOURCES})
target_link_libraries(Astra_Media
    PUBLIC
        Astra_Core
        Astra_Asset
        nlohmann_json::nlohmann_json
)

if(ASTRA_ENABLE_SDL_BACKEND)
    target_link_libraries(Astra_Media PRIVATE SDL3::SDL3)
    target_compile_definitions(Astra_Media PRIVATE ASTRA_ENABLE_SDL_BACKEND=1)
endif()
