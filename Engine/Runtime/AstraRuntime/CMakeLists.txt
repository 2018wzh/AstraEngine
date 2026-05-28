astra_add_library(Astra_AstraRuntime
    Private/AstraRuntimeSession.cpp
    Private/AstraScript.cpp
)

target_link_libraries(Astra_AstraRuntime
    PUBLIC
        Astra_AssetRegistry
        Astra_Bootstrap
        Astra_Core
        Astra_ExtensionRegistry
        Astra_ModuleRuntime
        Astra_VFS
        Astra_VNRuntimeServices
)
