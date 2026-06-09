set(ASTRA_RUNTIME_SOURCES
    Private/Runtime.cpp
)

astra_add_library(Astra_Runtime ${ASTRA_RUNTIME_SOURCES})
target_link_libraries(Astra_Runtime
    PUBLIC
        Astra_Core
        Astra_Scene
        nlohmann_json::nlohmann_json
)
