astra_add_library(Astra_Platform
    Private/PlatformServices.cpp
    Private/SdlPlatform.cpp
)

target_link_libraries(Astra_Platform
    PUBLIC
        Astra_Core
        SDL3::SDL3
)
