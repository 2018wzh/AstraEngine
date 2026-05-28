astra_add_library(Astra_PlatformSDL3
    Private/SdlPlatformPlugin.cpp
    Private/SdlPlatform.cpp
)

target_link_libraries(Astra_PlatformSDL3
    PUBLIC
        Astra_ApplicationCore
        Astra_Bootstrap
        Astra_Core
        SDL3::SDL3
)
