add_library(Astra_ExampleFoundationPlugin MODULE Source/Phase1Example.cpp)
astra_configure_target(Astra_ExampleFoundationPlugin)
target_link_libraries(Astra_ExampleFoundationPlugin PRIVATE Astra_ModuleRuntime)
set_target_properties(Astra_ExampleFoundationPlugin PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1Example/Bin/win64"
    LIBRARY_OUTPUT_DIRECTORY "${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1Example/Bin/win64"
)
file(GENERATE
    OUTPUT "${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1Example/Phase1Example.plugin.yaml"
    INPUT "${CMAKE_CURRENT_SOURCE_DIR}/Phase1Example.plugin.yaml.in"
)

add_library(Astra_InvalidNoEntryPlugin MODULE Source/InvalidNoEntry.cpp)
astra_configure_target(Astra_InvalidNoEntryPlugin)
set_target_properties(Astra_InvalidNoEntryPlugin PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1Invalid/Bin/win64"
    LIBRARY_OUTPUT_DIRECTORY "${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1Invalid/Bin/win64"
)

add_library(Astra_InvalidBadAbiPlugin MODULE Source/InvalidBadAbi.cpp)
astra_configure_target(Astra_InvalidBadAbiPlugin)
target_link_libraries(Astra_InvalidBadAbiPlugin PRIVATE Astra_ModuleRuntime)
set_target_properties(Astra_InvalidBadAbiPlugin PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1Invalid/Bin/win64"
    LIBRARY_OUTPUT_DIRECTORY "${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1Invalid/Bin/win64"
)
