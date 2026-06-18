set(ASTRA_SCRIPT_SOURCES
    Private/Script.cpp
    Private/NativeDslParser.cpp
    Private/ScriptExecution.cpp
    Private/ScriptLua.cpp
    Private/ScriptSerialization.cpp
)

astra_add_library(AstraScript ${ASTRA_SCRIPT_SOURCES})
target_link_libraries(AstraScript
    PUBLIC
        AstraCore
        AstraAsset
        AstraMedia
        AstraRuntime
        nlohmann_json::nlohmann_json
        taocpp::pegtl
    PRIVATE
        sol2::sol2
        unofficial::lua::lua
)
