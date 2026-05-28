astra_add_library(Astra_AssetCore
    Private/AssetId.cpp
)

target_link_libraries(Astra_AssetCore
    PUBLIC
        Astra_Core
)
