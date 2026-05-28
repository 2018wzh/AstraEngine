astra_add_library(Astra_Bootstrap
    Private/DefaultProjectContentPlugin.cpp
    Private/RuntimeProviderRegistry.cpp
)

target_link_libraries(Astra_Bootstrap
    PUBLIC
        Astra_ApplicationCore
        Astra_AssetRegistry
        Astra_Core
        Astra_VFS
        Astra_VNRuntimeServices
    PRIVATE
        yaml-cpp::yaml-cpp
)
