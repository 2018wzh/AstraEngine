astra_add_library(Astra_PropertySystem
    Private/PropertySystem.cpp
)

target_link_libraries(Astra_PropertySystem
    PUBLIC
        Astra_Core
        nlohmann_json::nlohmann_json
)
