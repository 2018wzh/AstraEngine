set(ASTRA_SCENE_SOURCES
    Private/Scene.cpp
)

astra_add_library(Astra_Scene ${ASTRA_SCENE_SOURCES})
target_link_libraries(Astra_Scene
    PUBLIC
        Astra_Core
        nlohmann_json::nlohmann_json
    PRIVATE
        EnTT::EnTT
)
