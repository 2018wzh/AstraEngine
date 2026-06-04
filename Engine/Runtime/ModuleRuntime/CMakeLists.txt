astra_add_library(Astra_ModuleRuntime
    Private/ExtensionRegistry.cpp
    Private/ModuleManager.cpp
    Private/PluginDescriptor.cpp
    Private/ServiceRegistry.cpp
)

target_link_libraries(Astra_ModuleRuntime
    PUBLIC
        Astra_Core
)
