set(ASTRA_MEDIA_SOURCES
    Private/Media.cpp
)

astra_add_library(AstraMedia ${ASTRA_MEDIA_SOURCES})
target_link_libraries(AstraMedia
    PUBLIC
        AstraCore
        AstraAsset
        nlohmann_json::nlohmann_json
)

if(ASTRA_ENABLE_SDL_BACKEND)
    target_link_libraries(AstraMedia PRIVATE SDL3::SDL3)
    target_compile_definitions(AstraMedia PRIVATE ASTRA_ENABLE_SDL_BACKEND=1)
endif()

find_package(PNG QUIET)
if(PNG_FOUND)
    target_link_libraries(AstraMedia PRIVATE PNG::PNG)
    target_compile_definitions(AstraMedia PRIVATE ASTRA_MEDIA_HAS_LIBPNG=1)
endif()

find_package(JPEG QUIET)
if(JPEG_FOUND)
    target_link_libraries(AstraMedia PRIVATE JPEG::JPEG)
    target_compile_definitions(AstraMedia PRIVATE ASTRA_MEDIA_HAS_LIBJPEG=1)
endif()

find_package(WebP CONFIG QUIET)
if(TARGET WebP::webp)
    target_link_libraries(AstraMedia PRIVATE WebP::webp)
    target_compile_definitions(AstraMedia PRIVATE ASTRA_MEDIA_HAS_LIBWEBP=1)
elseif(TARGET unofficial::webp::webp)
    target_link_libraries(AstraMedia PRIVATE unofficial::webp::webp)
    target_compile_definitions(AstraMedia PRIVATE ASTRA_MEDIA_HAS_LIBWEBP=1)
endif()

find_package(Freetype QUIET)
if(Freetype_FOUND)
    target_link_libraries(AstraMedia PRIVATE Freetype::Freetype)
    target_compile_definitions(AstraMedia PRIVATE ASTRA_MEDIA_HAS_FREETYPE=1)
endif()

find_package(harfbuzz CONFIG QUIET)
if(TARGET harfbuzz::harfbuzz)
    target_link_libraries(AstraMedia PRIVATE harfbuzz::harfbuzz)
    target_compile_definitions(AstraMedia PRIVATE ASTRA_MEDIA_HAS_HARFBUZZ=1)
endif()

find_path(MINIAUDIO_INCLUDE_DIRS "miniaudio.h")
if(MINIAUDIO_INCLUDE_DIRS)
    target_include_directories(AstraMedia PRIVATE ${MINIAUDIO_INCLUDE_DIRS})
    target_compile_definitions(AstraMedia PRIVATE ASTRA_MEDIA_HAS_MINIAUDIO=1)
endif()

find_package(FFMPEG QUIET)
if(FFMPEG_FOUND)
    target_include_directories(AstraMedia PRIVATE ${FFMPEG_INCLUDE_DIRS})
    target_link_directories(AstraMedia PRIVATE ${FFMPEG_LIBRARY_DIRS})
    target_link_libraries(AstraMedia PRIVATE ${FFMPEG_LIBRARIES})
    target_compile_definitions(AstraMedia PRIVATE ASTRA_MEDIA_HAS_FFMPEG=1)
endif()

find_package(Vorbis CONFIG QUIET)
if(TARGET Vorbis::vorbisfile)
    target_link_libraries(AstraMedia PRIVATE Vorbis::vorbisfile)
    target_compile_definitions(AstraMedia PRIVATE ASTRA_MEDIA_HAS_VORBISFILE=1)
endif()
