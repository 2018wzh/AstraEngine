add_executable(AstraPhaseTests
    PhaseTests.cpp
)
astra_configure_target(AstraPhaseTests)
target_link_libraries(AstraPhaseTests
    PRIVATE
        AstraCore
        AstraAsset
        AstraMedia
        AstraPlatform
        AstraPropertySystem
        AstraModuleRuntime
        AstraScene
        AstraRuntime
        AstraScript
        AstraVN
        Catch2::Catch2WithMain
)
if(TARGET AstraTools)
    target_link_libraries(AstraPhaseTests PRIVATE AstraTools)
    target_compile_definitions(AstraPhaseTests PRIVATE ASTRA_WITH_TOOLS=1)
endif()
target_compile_definitions(AstraPhaseTests
    PRIVATE
        ASTRA_SOURCE_ROOT="${CMAKE_SOURCE_DIR}"
)

add_test(NAME AstraPhaseTests COMMAND AstraPhaseTests)

if(TARGET astra)
    add_test(NAME AstraCliVersion COMMAND astra --version)
    add_test(NAME AstraCliDocCheck COMMAND astra doc-check)
    add_test(NAME AstraCliValidatePackageSmoke COMMAND astra validate "${CMAKE_SOURCE_DIR}/Samples/PackageSmoke" --strict)
    add_test(NAME AstraCliValidateNativeVN COMMAND astra validate "${CMAKE_SOURCE_DIR}/Samples/NativeVN" --strict)
    add_test(NAME AstraCliValidateNativeVNJson COMMAND astra validate "${CMAKE_SOURCE_DIR}/Samples/NativeVN" --strict --json)
    add_test(NAME AstraCliPackageSmoke COMMAND astra package "${CMAKE_SOURCE_DIR}/Samples/PackageSmoke" --profile development)
    add_test(NAME AstraCliRunPackageSmoke COMMAND astra run "${CMAKE_SOURCE_DIR}/Samples/PackageSmoke" --headless-smoke)
    add_test(NAME AstraCliRunPackageSmokeJson COMMAND astra run "${CMAKE_SOURCE_DIR}/Samples/PackageSmoke" --headless-smoke --json)
    add_test(NAME AstraCliPackageNativeVN COMMAND astra package "${CMAKE_SOURCE_DIR}/Samples/NativeVN" --profile deterministic --json)
    add_test(NAME AstraCliRunNativeVNPackage COMMAND astra run "${CMAKE_BINARY_DIR}/Saved/Packages/NativeVN.astrapkg" --headless-smoke --json)
    add_test(NAME AstraCliReplayNativeVN COMMAND astra replay "${CMAKE_BINARY_DIR}/Saved/Replays/NativeVNGolden.replay" --compare --json)
    set_tests_properties(AstraCliValidateNativeVNJson PROPERTIES PASS_REGULAR_EXPRESSION "phase4_script_vn")
    set_tests_properties(AstraCliRunPackageSmokeJson PROPERTIES PASS_REGULAR_EXPRESSION "media_release_gate")
    set_tests_properties(AstraCliPackageNativeVN PROPERTIES PASS_REGULAR_EXPRESSION "golden_replay")
    set_tests_properties(AstraCliRunNativeVNPackage PROPERTIES PASS_REGULAR_EXPRESSION "playable_vn")
    set_tests_properties(AstraCliReplayNativeVN PROPERTIES PASS_REGULAR_EXPRESSION "comparison")
    set_tests_properties(AstraCliRunNativeVNPackage AstraCliReplayNativeVN PROPERTIES DEPENDS AstraCliPackageNativeVN)
    if(EXISTS "${CMAKE_SOURCE_DIR}/Samples/TsuiNoSora/astra.sample.yaml")
        add_test(NAME AstraCliValidateTsuiNoSoraJson COMMAND astra validate "${CMAKE_SOURCE_DIR}/Samples/TsuiNoSora" --strict --json)
        add_test(NAME AstraCliPackageTsuiNoSora COMMAND astra package "${CMAKE_SOURCE_DIR}/Samples/TsuiNoSora" --profile development --json)
        add_test(NAME AstraCliRunTsuiNoSoraPackage COMMAND astra run "${CMAKE_BINARY_DIR}/Saved/Packages/TsuiNoSora.astrapkg" --headless-smoke --json)
        add_test(NAME AstraCliReplayTsuiNoSora COMMAND astra replay "${CMAKE_BINARY_DIR}/Saved/Replays/TsuiNoSoraGolden.replay" --compare --json)
        set_tests_properties(AstraCliValidateTsuiNoSoraJson PROPERTIES PASS_REGULAR_EXPRESSION "native:/Fonts/SourceHanSerif")
        set_tests_properties(AstraCliPackageTsuiNoSora PROPERTIES PASS_REGULAR_EXPRESSION "golden_replay")
        set_tests_properties(AstraCliRunTsuiNoSoraPackage PROPERTIES PASS_REGULAR_EXPRESSION "system_ui_state")
        set_tests_properties(AstraCliReplayTsuiNoSora PROPERTIES PASS_REGULAR_EXPRESSION "comparison")
        set_tests_properties(AstraCliRunTsuiNoSoraPackage AstraCliReplayTsuiNoSora PROPERTIES DEPENDS AstraCliPackageTsuiNoSora)
    endif()
endif()

if(TARGET AstraExampleFoundationPlugin)
    add_dependencies(AstraPhaseTests AstraExampleFoundationPlugin AstraInvalidNoEntryPlugin AstraInvalidBadAbiPlugin)
    target_compile_definitions(AstraPhaseTests
        PRIVATE
            ASTRA_PHASE1_PLUGIN_DESCRIPTOR="${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1Example/Phase1Example.plugin.yaml"
            ASTRA_PHASE1_INVALID_PLUGIN_ROOT="${ASTRA_PLUGIN_OUTPUT_ROOT}/Phase1Invalid"
            ASTRA_PHASE1_INVALID_NO_ENTRY="$<TARGET_FILE_NAME:AstraInvalidNoEntryPlugin>"
            ASTRA_PHASE1_INVALID_BAD_ABI="$<TARGET_FILE_NAME:AstraInvalidBadAbiPlugin>"
    )
endif()
