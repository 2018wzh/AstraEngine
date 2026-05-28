add_executable(Astra_RuntimeTests
    RuntimeTests.cpp
)

astra_configure_target(Astra_RuntimeTests)

target_link_libraries(Astra_RuntimeTests
    PRIVATE
        Astra_AstraRuntime
        Astra_AssetRegistry
        Astra_Bootstrap
        Astra_ExtensionRegistry
        Astra_ModuleRuntime
        Astra_VNPropertySystem
        Catch2::Catch2WithMain
)

target_compile_definitions(Astra_RuntimeTests
    PRIVATE
        ASTRA_TEST_PLUGIN_ROOT="${ASTRA_PLUGIN_OUTPUT_ROOT}"
        ASTRA_TEST_PROJECT_ROOT="${ASTRA_SAMPLE_PROJECT_ROOT}"
)

add_dependencies(Astra_RuntimeTests
    Astra_DefaultRuntimeProvidersPlugin
    Astra_ExampleRuntimePlugin
)
add_test(NAME Astra_RuntimeTests COMMAND Astra_RuntimeTests)
