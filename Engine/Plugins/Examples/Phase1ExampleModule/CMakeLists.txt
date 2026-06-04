astra_add_module(Astra_Phase1ExampleModule
    Private/Phase1ExampleModule.cpp
)

target_link_libraries(Astra_Phase1ExampleModule
    PRIVATE
        Astra_Core
        Astra_ModuleRuntime
        Astra_Platform
)

target_compile_definitions(Astra_Phase1ExampleModule
    PRIVATE
        ASTRA_MODULE_BUILD=1
)

set_target_properties(Astra_Phase1ExampleModule PROPERTIES
    PREFIX ""
    OUTPUT_NAME "Phase1ExampleModule"
    LIBRARY_OUTPUT_DIRECTORY "${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1ExampleModule"
    RUNTIME_OUTPUT_DIRECTORY "${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1ExampleModule"
)

add_custom_command(TARGET Astra_Phase1ExampleModule POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
        "${CMAKE_CURRENT_SOURCE_DIR}/Phase1ExampleModule.plugin.yaml"
        "${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1ExampleModule/Phase1ExampleModule.plugin.yaml"
)
