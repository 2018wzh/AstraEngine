set(ASTRA_TOOLS_SOURCES
    Private/Tools.cpp
    Private/DocCheck.cpp
    Private/ToolsHash.cpp
)

astra_add_library(AstraTools ${ASTRA_TOOLS_SOURCES})
target_link_libraries(AstraTools
    PUBLIC
        AstraCore
        AstraAsset
        AstraMedia
        AstraModuleRuntime
        AstraPropertySystem
        AstraScript
        AstraVN
        AstraPlatform
        nlohmann_json::nlohmann_json
    PRIVATE
        OpenSSL::Crypto
        yaml-cpp::yaml-cpp
)
target_compile_definitions(AstraTools
    PRIVATE
        ASTRA_SOURCE_ROOT="${CMAKE_SOURCE_DIR}"
        ASTRA_BINARY_ROOT="${CMAKE_BINARY_DIR}"
)

add_executable(astra Private/main.cpp)
astra_configure_target(astra)
target_link_libraries(astra PRIVATE AstraTools CLI11::CLI11)
