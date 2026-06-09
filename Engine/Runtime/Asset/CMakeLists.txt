set(ASTRA_ASSET_SOURCES
    Private/Asset.cpp
)

astra_add_library(Astra_Asset ${ASTRA_ASSET_SOURCES})
target_link_libraries(Astra_Asset
    PUBLIC
        Astra_Core
        Astra_Platform
        nlohmann_json::nlohmann_json
    PRIVATE
        yaml-cpp::yaml-cpp
)
