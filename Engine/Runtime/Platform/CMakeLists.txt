set(ASTRA_PLATFORM_SOURCES
    Private/Platform.cpp
)

if(ASTRA_ENABLE_SDL_BACKEND)
    list(APPEND ASTRA_PLATFORM_SOURCES Private/SdlPlatform.cpp)
endif()

astra_add_library(Astra_Platform ${ASTRA_PLATFORM_SOURCES})
target_link_libraries(Astra_Platform PUBLIC Astra_Core)
target_compile_definitions(Astra_Platform
    PUBLIC
        ASTRA_ENABLE_HEADLESS_BACKEND=$<BOOL:${ASTRA_ENABLE_HEADLESS_BACKEND}>
        ASTRA_ENABLE_SDL_BACKEND=$<BOOL:${ASTRA_ENABLE_SDL_BACKEND}>
)

if(ASTRA_ENABLE_SDL_BACKEND)
    target_link_libraries(Astra_Platform PRIVATE SDL3::SDL3)
endif()
