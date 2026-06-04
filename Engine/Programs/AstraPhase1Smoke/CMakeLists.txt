add_executable(AstraPhase1Smoke
    Private/main.cpp
)

astra_configure_target(AstraPhase1Smoke)

target_link_libraries(AstraPhase1Smoke
    PRIVATE
        Astra_Core
        Astra_ModuleRuntime
        Astra_Platform
        Astra_PropertySystem
)

target_compile_definitions(AstraPhase1Smoke
    PRIVATE
        ASTRA_DEFAULT_PLUGIN_ROOT="${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1ExampleModule"
)

add_dependencies(AstraPhase1Smoke
    Astra_Phase1ExampleModule
)
