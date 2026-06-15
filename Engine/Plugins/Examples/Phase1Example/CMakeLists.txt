add_library(AstraExampleFoundationPlugin MODULE Source/Phase1Example.cpp)
astra_configure_target(AstraExampleFoundationPlugin)
target_link_libraries(AstraExampleFoundationPlugin PRIVATE AstraModuleRuntime)
set_target_properties(AstraExampleFoundationPlugin PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1Example/Bin/win64"
    LIBRARY_OUTPUT_DIRECTORY "${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1Example/Bin/win64"
)
file(GENERATE
    OUTPUT "${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1Example/Phase1Example.plugin.yaml"
    INPUT "${CMAKE_CURRENT_SOURCE_DIR}/Phase1Example.plugin.yaml.in"
)

add_library(AstraInvalidNoEntryPlugin MODULE Source/InvalidNoEntry.cpp)
astra_configure_target(AstraInvalidNoEntryPlugin)
target_link_libraries(AstraInvalidNoEntryPlugin PRIVATE AstraModuleRuntime)
set_target_properties(AstraInvalidNoEntryPlugin PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1Invalid/Bin/win64"
    LIBRARY_OUTPUT_DIRECTORY "${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1Invalid/Bin/win64"
)

add_library(AstraInvalidBadAbiPlugin MODULE Source/InvalidBadAbi.cpp)
astra_configure_target(AstraInvalidBadAbiPlugin)
target_link_libraries(AstraInvalidBadAbiPlugin PRIVATE AstraModuleRuntime)
set_target_properties(AstraInvalidBadAbiPlugin PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1Invalid/Bin/win64"
    LIBRARY_OUTPUT_DIRECTORY "${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1Invalid/Bin/win64"
)
