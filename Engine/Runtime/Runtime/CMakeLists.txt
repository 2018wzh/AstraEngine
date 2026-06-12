set(ASTRA_RUNTIME_SOURCES
    Private/Runtime.cpp
)

astra_add_library(AstraRuntime ${ASTRA_RUNTIME_SOURCES})
target_link_libraries(AstraRuntime
    PUBLIC
        AstraCore
        AstraScene
        nlohmann_json::nlohmann_json
    PRIVATE
        zstd::libzstd
)
