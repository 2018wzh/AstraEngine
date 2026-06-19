set(ASTRA_GAME_SOURCES
    Private/AstraGame.cpp
)

astra_add_library(AstraGame ${ASTRA_GAME_SOURCES})
target_link_libraries(AstraGame
    PUBLIC
        AstraCore
        AstraAsset
        AstraPlatform
        AstraModuleRuntime
        AstraRuntime
        AstraScript
        AstraVN
        nlohmann_json::nlohmann_json
)
