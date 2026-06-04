add_executable(Astra_Phase1Tests
    Phase1Tests.cpp
)

astra_configure_target(Astra_Phase1Tests)

target_link_libraries(Astra_Phase1Tests
    PRIVATE
        Astra_Core
        Astra_ModuleRuntime
        Astra_Platform
        Astra_PropertySystem
        Catch2::Catch2WithMain
)

target_compile_definitions(Astra_Phase1Tests
    PRIVATE
        ASTRA_TEST_PLUGIN_ROOT="${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1ExampleModule"
)

add_dependencies(Astra_Phase1Tests
    Astra_Phase1ExampleModule
)

add_test(NAME Astra_Phase1Tests COMMAND Astra_Phase1Tests)
