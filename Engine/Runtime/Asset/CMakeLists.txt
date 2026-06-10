set(ASTRA_ASSET_SOURCES
    Private/Asset.cpp
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
)
