set(ASTRA_SCRIPT_SOURCES
    Private/Script.cpp
)

astra_add_library(AstraScript ${ASTRA_SCRIPT_SOURCES})
target_link_libraries(AstraScript
    PUBLIC
        AstraCore
        AstraAsset
        AstraMedia
        AstraRuntime
        nlohmann_json::nlohmann_json
    PRIVATE
        sol2::sol2
        unofficial::lua::lua
)
