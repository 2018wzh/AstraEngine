add_executable(Astra_Phase1Tests
    Phase1Tests.cpp
)
astra_configure_target(Astra_Phase1Tests)
target_link_libraries(Astra_Phase1Tests
    PRIVATE
        Astra_Core
        Astra_Asset
        Astra_Media
        Astra_Platform
        Astra_PropertySystem
        Astra_ModuleRuntime
        Astra_Scene
        Astra_Runtime
        Astra_Script
        Astra_AstraVN
        Catch2::Catch2WithMain
)
if(TARGET Astra_Tools)
    target_link_libraries(Astra_Phase1Tests PRIVATE Astra_Tools)
    target_compile_definitions(Astra_Phase1Tests PRIVATE ASTRA_WITH_TOOLS=1)
endif()
target_compile_definitions(Astra_Phase1Tests
    PRIVATE
        ASTRA_SOURCE_ROOT="${CMAKE_SOURCE_DIR}"
)

add_test(NAME Astra_Phase1Tests COMMAND Astra_Phase1Tests)

if(TARGET astra)
    add_test(NAME Astra_CliVersion COMMAND astra --version)
    add_test(NAME Astra_CliDocCheck COMMAND astra doc-check)
    add_test(NAME Astra_CliValidatePackageSmoke COMMAND astra validate "${CMAKE_SOURCE_DIR}/Samples/PackageSmoke" --strict)
    add_test(NAME Astra_CliValidateNativeVN COMMAND astra validate "${CMAKE_SOURCE_DIR}/Samples/NativeVN" --strict)
    add_test(NAME Astra_CliValidateNativeVNJson COMMAND astra validate "${CMAKE_SOURCE_DIR}/Samples/NativeVN" --strict --json)
    add_test(NAME Astra_CliPackageSmoke COMMAND astra package "${CMAKE_SOURCE_DIR}/Samples/PackageSmoke" --profile development)
    add_test(NAME Astra_CliRunPackageSmoke COMMAND astra run "${CMAKE_SOURCE_DIR}/Samples/PackageSmoke" --headless-smoke)
    add_test(NAME Astra_CliRunPackageSmokeJson COMMAND astra run "${CMAKE_SOURCE_DIR}/Samples/PackageSmoke" --headless-smoke --json)
    set_tests_properties(Astra_CliValidateNativeVNJson PROPERTIES PASS_REGULAR_EXPRESSION "phase4_script_vn")
    set_tests_properties(Astra_CliRunPackageSmokeJson PROPERTIES PASS_REGULAR_EXPRESSION "media_release_gate")
endif()

if(TARGET Astra_ExampleFoundationPlugin)
    add_dependencies(Astra_Phase1Tests Astra_ExampleFoundationPlugin Astra_InvalidNoEntryPlugin Astra_InvalidBadAbiPlugin)
    target_compile_definitions(Astra_Phase1Tests
        PRIVATE
            ASTRA_PHASE1_PLUGIN_DESCRIPTOR="${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1Example/Phase1Example.plugin.yaml"
            ASTRA_PHASE1_INVALID_PLUGIN_ROOT="${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1Invalid"
            ASTRA_PHASE1_INVALID_NO_ENTRY="$<TARGET_FILE_NAME:Astra_InvalidNoEntryPlugin>"
            ASTRA_PHASE1_INVALID_BAD_ABI="$<TARGET_FILE_NAME:Astra_InvalidBadAbiPlugin>"
    )
endif()
