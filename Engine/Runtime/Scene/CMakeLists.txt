set(ASTRA_SCENE_SOURCES
    Private/Scene.cpp
)

astra_add_library(AstraScene ${ASTRA_SCENE_SOURCES})
target_link_libraries(AstraScene
    PUBLIC
        AstraCore
        nlohmann_json::nlohmann_json
    PRIVATE
        EnTT::EnTT
)
