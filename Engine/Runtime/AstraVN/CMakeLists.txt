set(ASTRA_ASTRAVN_SOURCES
    Private/AstraVN.cpp
)

astra_add_library(AstraVN ${ASTRA_ASTRAVN_SOURCES})
target_link_libraries(AstraVN
    PUBLIC
        AstraCore
        AstraAsset
        AstraMedia
        AstraRuntime
        AstraScript
        nlohmann_json::nlohmann_json
)
