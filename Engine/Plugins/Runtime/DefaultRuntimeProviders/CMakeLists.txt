add_library(Astra_DefaultRuntimeProvidersPlugin MODULE
    Private/DefaultRuntimeProvidersPlugin.cpp
)

astra_configure_target(Astra_DefaultRuntimeProvidersPlugin)
target_link_libraries(Astra_DefaultRuntimeProvidersPlugin
    PRIVATE
        Astra_AudioCore
        Astra_Bootstrap
        Astra_PlatformSDL3
        Astra_Renderer2D
)
target_include_directories(Astra_DefaultRuntimeProvidersPlugin
    PRIVATE
        "${CMAKE_SOURCE_DIR}/Engine/Runtime/ModuleRuntime/Public"
)
target_compile_definitions(Astra_DefaultRuntimeProvidersPlugin
    PRIVATE
        ASTRA_MODULE_BUILD=1
)
set_target_properties(Astra_DefaultRuntimeProvidersPlugin PROPERTIES
    PREFIX ""
    OUTPUT_NAME "DefaultRuntimeProvidersPlugin"
    RUNTIME_OUTPUT_DIRECTORY "${ASTRA_PLUGIN_OUTPUT_ROOT}/DefaultRuntimeProviders/Bin/win64"
    LIBRARY_OUTPUT_DIRECTORY "${ASTRA_PLUGIN_OUTPUT_ROOT}/DefaultRuntimeProviders/Bin/win64"
)

add_custom_command(TARGET Astra_DefaultRuntimeProvidersPlugin POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E make_directory "${ASTRA_PLUGIN_OUTPUT_ROOT}/DefaultRuntimeProviders"
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
        "${CMAKE_CURRENT_SOURCE_DIR}/DefaultRuntimeProviders.plugin.yaml"
        "${ASTRA_PLUGIN_OUTPUT_ROOT}/DefaultRuntimeProviders/DefaultRuntimeProviders.plugin.yaml"
)
