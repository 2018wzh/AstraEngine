set(ASTRA_SCRIPT_SOURCES
    Private/Script.cpp
)

astra_add_library(Astra_Script ${ASTRA_SCRIPT_SOURCES})
target_link_libraries(Astra_Script
    PUBLIC
        Astra_Core
        Astra_Asset
        Astra_Media
        Astra_Runtime
        nlohmann_json::nlohmann_json
    PRIVATE
        sol2::sol2
        unofficial::lua::lua
)
