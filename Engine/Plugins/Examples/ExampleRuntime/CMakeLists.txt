add_library(Astra_ExampleRuntimePlugin MODULE
    Private/ExampleRuntimePlugin.cpp
)

astra_configure_target(Astra_ExampleRuntimePlugin)
target_include_directories(Astra_ExampleRuntimePlugin
    PRIVATE
        "${CMAKE_SOURCE_DIR}/Engine/Runtime/ModuleRuntime/Public"
)
target_compile_definitions(Astra_ExampleRuntimePlugin
    PRIVATE
        ASTRA_MODULE_BUILD=1
)
set_target_properties(Astra_ExampleRuntimePlugin PROPERTIES
    PREFIX ""
    OUTPUT_NAME "ExampleRuntimePlugin"
    RUNTIME_OUTPUT_DIRECTORY "${ASTRA_PLUGIN_OUTPUT_ROOT}/ExampleRuntime/Bin/win64"
    LIBRARY_OUTPUT_DIRECTORY "${ASTRA_PLUGIN_OUTPUT_ROOT}/ExampleRuntime/Bin/win64"
)

add_custom_command(TARGET Astra_ExampleRuntimePlugin POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E make_directory "${ASTRA_PLUGIN_OUTPUT_ROOT}/ExampleRuntime"
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
        "${CMAKE_CURRENT_SOURCE_DIR}/ExampleRuntime.plugin.yaml"
        "${ASTRA_PLUGIN_OUTPUT_ROOT}/ExampleRuntime/ExampleRuntime.plugin.yaml"
)
