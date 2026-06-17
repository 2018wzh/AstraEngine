set(ASTRA_ASSET_SOURCES
    Private/Asset.cpp
    Private/AssetCook.cpp
    Private/AssetMetadata.cpp
    Private/AssetSerialization.cpp
)

astra_add_library(AstraAsset ${ASTRA_ASSET_SOURCES})
target_link_libraries(AstraAsset
    PUBLIC
        AstraCore
        AstraPlatform
        nlohmann_json::nlohmann_json
    PRIVATE
        OpenSSL::Crypto
        yaml-cpp::yaml-cpp
        zstd::libzstd
)

find_package(PNG QUIET)
if(PNG_FOUND)
    target_link_libraries(AstraAsset PRIVATE PNG::PNG)
    target_compile_definitions(AstraAsset PRIVATE ASTRA_ASSET_HAS_LIBPNG=1)
endif()

find_package(JPEG QUIET)
if(JPEG_FOUND)
    target_link_libraries(AstraAsset PRIVATE JPEG::JPEG)
    target_compile_definitions(AstraAsset PRIVATE ASTRA_ASSET_HAS_LIBJPEG=1)
endif()

find_package(WebP CONFIG QUIET)
if(TARGET WebP::webp)
    target_link_libraries(AstraAsset PRIVATE WebP::webp)
    target_compile_definitions(AstraAsset PRIVATE ASTRA_ASSET_HAS_LIBWEBP=1)
elseif(TARGET unofficial::webp::webp)
    target_link_libraries(AstraAsset PRIVATE unofficial::webp::webp)
    target_compile_definitions(AstraAsset PRIVATE ASTRA_ASSET_HAS_LIBWEBP=1)
endif()

find_package(Freetype QUIET)
if(Freetype_FOUND)
    target_link_libraries(AstraAsset PRIVATE Freetype::Freetype)
    target_compile_definitions(AstraAsset PRIVATE ASTRA_ASSET_HAS_FREETYPE=1)
endif()

find_package(harfbuzz CONFIG QUIET)
if(TARGET harfbuzz::harfbuzz)
    target_link_libraries(AstraAsset PRIVATE harfbuzz::harfbuzz)
    target_compile_definitions(AstraAsset PRIVATE ASTRA_ASSET_HAS_HARFBUZZ=1)
endif()

find_path(MINIAUDIO_INCLUDE_DIRS "miniaudio.h")
if(MINIAUDIO_INCLUDE_DIRS)
    target_include_directories(AstraAsset PRIVATE ${MINIAUDIO_INCLUDE_DIRS})
    target_compile_definitions(AstraAsset PRIVATE ASTRA_ASSET_HAS_MINIAUDIO=1)
endif()
