astra_add_library(Astra_AssetRegistry
    Private/AssetRegistry.cpp
)

target_link_libraries(Astra_AssetRegistry
    PUBLIC
        Astra_AssetCore
        Astra_Core
    PRIVATE
        yaml-cpp::yaml-cpp
)
