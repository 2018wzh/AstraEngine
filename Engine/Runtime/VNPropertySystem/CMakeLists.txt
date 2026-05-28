astra_add_library(Astra_VNPropertySystem
    Private/VNPropertySystem.cpp
)

target_link_libraries(Astra_VNPropertySystem
    PUBLIC
        Astra_Core
        nlohmann_json::nlohmann_json
)
