set(ASTRA_PLATFORM_SOURCES
    Private/Platform.cpp
    Private/Target/AndroidTarget.cpp
    Private/Target/HeadlessTarget.cpp
    Private/Target/IosTarget.cpp
    Private/Target/LinuxTarget.cpp
    Private/Target/MacOsTarget.cpp
    Private/Target/TargetPlatform.cpp
    Private/Target/WebTarget.cpp
    Private/Target/WindowsTarget.cpp
)

if(ASTRA_ENABLE_SDL_BACKEND)
    list(APPEND ASTRA_PLATFORM_SOURCES Private/SdlPlatform.cpp)
endif()

astra_add_library(AstraPlatform ${ASTRA_PLATFORM_SOURCES})
target_link_libraries(AstraPlatform PUBLIC AstraCore)
target_compile_definitions(AstraPlatform
    PUBLIC
        ASTRA_ENABLE_HEADLESS_BACKEND=$<BOOL:${ASTRA_ENABLE_HEADLESS_BACKEND}>
        ASTRA_ENABLE_SDL_BACKEND=$<BOOL:${ASTRA_ENABLE_SDL_BACKEND}>
)

if(ASTRA_ENABLE_SDL_BACKEND)
    target_link_libraries(AstraPlatform PRIVATE SDL3::SDL3)
endif()

astra_add_library(AstraPlatformHeadless Private/BackendAnchor.cpp)
target_link_libraries(AstraPlatformHeadless PUBLIC AstraCore AstraPlatform)

astra_add_library(AstraPlatformMobileStub Private/BackendAnchor.cpp)
target_link_libraries(AstraPlatformMobileStub PUBLIC AstraCore AstraPlatform)

astra_add_library(AstraPlatformWebStub Private/BackendAnchor.cpp)
target_link_libraries(AstraPlatformWebStub PUBLIC AstraCore AstraPlatform)

if(ASTRA_ENABLE_SDL_BACKEND)
    astra_add_library(AstraPlatformDesktopSdl Private/BackendAnchor.cpp Private/SdlPlatform.cpp)
    target_link_libraries(AstraPlatformDesktopSdl PUBLIC AstraCore AstraPlatform PRIVATE SDL3::SDL3)
    target_compile_definitions(AstraPlatformDesktopSdl PRIVATE ASTRA_ENABLE_SDL_BACKEND=1)
endif()
