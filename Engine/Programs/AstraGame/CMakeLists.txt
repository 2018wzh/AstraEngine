add_executable(AstraGame
    Private/main.cpp
)

astra_configure_target(AstraGame)

target_link_libraries(AstraGame
    PRIVATE
        Astra_AstraRuntime
        Astra_Bootstrap
        Astra_Core
        Astra_ExtensionRegistry
        Astra_ModuleRuntime
)

target_compile_definitions(AstraGame
    PRIVATE
        ASTRA_DEFAULT_PLUGIN_ROOT="${ASTRA_PLUGIN_OUTPUT_ROOT}"
        ASTRA_SAMPLE_PROJECT_ROOT="${ASTRA_SAMPLE_PROJECT_ROOT}"
)

add_dependencies(AstraGame
    Astra_DefaultRuntimeProvidersPlugin
    Astra_ExampleRuntimePlugin
)
