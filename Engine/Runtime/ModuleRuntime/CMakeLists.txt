astra_add_library(Astra_ModuleRuntime
    Private/ModuleManager.cpp
    Private/PluginDescriptor.cpp
)

target_link_libraries(Astra_ModuleRuntime
    PUBLIC
        Astra_Bootstrap
        Astra_Core
        Astra_ExtensionRegistry
    PRIVATE
        yaml-cpp::yaml-cpp
)

target_include_directories(Astra_ModuleRuntime
    PUBLIC
        "${CMAKE_CURRENT_SOURCE_DIR}/Public"
)
