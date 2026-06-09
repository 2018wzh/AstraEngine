set(ASTRA_TOOLS_SOURCES
    Private/Tools.cpp
)

astra_add_library(Astra_Tools ${ASTRA_TOOLS_SOURCES})
target_link_libraries(Astra_Tools
    PUBLIC
        Astra_Core
        Astra_Asset
        Astra_Media
        Astra_ModuleRuntime
        Astra_PropertySystem
        Astra_Script
        Astra_AstraVN
        Astra_Platform
        nlohmann_json::nlohmann_json
    PRIVATE
        OpenSSL::Crypto
        yaml-cpp::yaml-cpp
)
target_compile_definitions(Astra_Tools
    PRIVATE
        ASTRA_SOURCE_ROOT="${CMAKE_SOURCE_DIR}"
        ASTRA_BINARY_ROOT="${CMAKE_BINARY_DIR}"
)

add_executable(astra Private/main.cpp)
astra_configure_target(astra)
target_link_libraries(astra PRIVATE Astra_Tools CLI11::CLI11)
