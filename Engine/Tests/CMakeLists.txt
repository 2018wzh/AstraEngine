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
    add_test(NAME AstraCliValidateNativeVNLogs COMMAND astra validate "${CMAKE_SOURCE_DIR}/Samples/NativeVN" --strict --json --log-dir "${CMAKE_BINARY_DIR}/Saved/TestLogs" --log-level trace)
    add_test(NAME AstraCliPackageSmoke COMMAND astra package "${CMAKE_SOURCE_DIR}/Samples/PackageSmoke" --profile development)
    add_test(NAME AstraCliRunPackageSmoke COMMAND astra run "${CMAKE_SOURCE_DIR}/Samples/PackageSmoke" --headless-smoke)
    add_test(NAME AstraCliRunPackageSmokeJson COMMAND astra run "${CMAKE_SOURCE_DIR}/Samples/PackageSmoke" --headless-smoke --json)
    add_test(NAME AstraCliPackageNativeVN COMMAND astra package "${CMAKE_SOURCE_DIR}/Samples/NativeVN" --profile deterministic --json)
    if(WIN32)
        add_test(NAME AstraCliPackageNativeVNWin64Distribution COMMAND powershell -NoProfile -ExecutionPolicy Bypass -Command "$root = '${CMAKE_BINARY_DIR}/Saved/Releases/NativeVN/NativeVN-win64'; $required = @('astra.exe','distribution-manifest.json','Packages/NativeVN.astrapkg','Plugins/Phase1Example/Phase1Example.plugin.yaml','Plugins/Phase1Example/Bin/win64/AstraExampleFoundationPlugin.dll'); foreach ($item in $required) { if (!(Test-Path (Join-Path $root $item))) { Write-Output \"missing $item\"; exit 1 } }; 'distribution-manifest.json'")
        add_test(NAME AstraCliPackageNativeVNLinuxDistributionMissing COMMAND powershell -NoProfile -ExecutionPolicy Bypass -Command "$out = & '$<TARGET_FILE:astra>' package '${CMAKE_SOURCE_DIR}/Samples/NativeVN' --profile deterministic --target-platform linux-x64 --json 2>&1; if ($LASTEXITCODE -eq 0) { $out; exit 1 }; $text = $out -join [Environment]::NewLine; if ($text -notmatch 'ASTRA_DISTRIBUTION_TARGET_BINARIES_MISSING') { $out; exit 1 }; 'ASTRA_DISTRIBUTION_TARGET_BINARIES_MISSING'")
    endif()
    add_test(NAME AstraCliReleaseGateNativeVN COMMAND astra release-gate "${CMAKE_SOURCE_DIR}/Samples/NativeVN" --profile deterministic --json)
    add_test(NAME AstraCliRunNativeVNPackage COMMAND astra run "${CMAKE_BINARY_DIR}/Saved/Packages/NativeVN.astrapkg" --headless-smoke --json)
    add_test(NAME AstraCliRunNativeVNSave COMMAND astra run "${CMAKE_BINARY_DIR}/Saved/Packages/NativeVN.astrapkg" --headless-smoke --save-out "${CMAKE_BINARY_DIR}/Saved/Saves/NativeVN.ctest.save.json" --json)
    add_test(NAME AstraCliRunNativeVNLoad COMMAND astra run "${CMAKE_BINARY_DIR}/Saved/Packages/NativeVN.astrapkg" --load "${CMAKE_BINARY_DIR}/Saved/Saves/NativeVN.ctest.save.json" --headless-smoke --json)
    add_test(NAME AstraCliRunNativeVNWindowedScripted COMMAND astra run "${CMAKE_BINARY_DIR}/Saved/Packages/NativeVN.astrapkg" --windowed-smoke --scripted-input "${CMAKE_SOURCE_DIR}/Samples/NativeVN/Input/golden.yaml" --auto-close --json)
    add_test(NAME AstraCliRunNativeVNQaTitleWalk COMMAND astra run "${CMAKE_BINARY_DIR}/Saved/Packages/NativeVN.astrapkg" --windowed-smoke --scripted-input "${CMAKE_SOURCE_DIR}/Samples/NativeVN/Input/qa/title_walk.yaml" --auto-close --json)
    add_test(NAME AstraCliRunNativeVNQaSystemSaveLoad COMMAND astra run "${CMAKE_BINARY_DIR}/Saved/Packages/NativeVN.astrapkg" --windowed-smoke --scripted-input "${CMAKE_SOURCE_DIR}/Samples/NativeVN/Input/qa/system_save_load_systems.yaml" --auto-close --json)
    add_test(NAME AstraCliRunNativeVNQaMenuReopen COMMAND astra run "${CMAKE_BINARY_DIR}/Saved/Packages/NativeVN.astrapkg" --windowed-smoke --scripted-input "${CMAKE_SOURCE_DIR}/Samples/NativeVN/Input/qa/menu_reopen_smoke.yaml" --auto-close --json)
    add_test(NAME AstraCliRunNativeVNMissingSave COMMAND powershell -NoProfile -ExecutionPolicy Bypass -Command "$out = & '$<TARGET_FILE:astra>' run '${CMAKE_BINARY_DIR}/Saved/Packages/NativeVN.astrapkg' --headless-smoke --load '${CMAKE_BINARY_DIR}/Saved/Saves/NativeVN.missing.save.json' --json 2>&1; if ($LASTEXITCODE -eq 0) { $out; exit 1 }; $text = $out -join [Environment]::NewLine; if ($text -notmatch 'ASTRA_RUN_SAVE_MISSING') { $out; exit 1 }; 'ASTRA_RUN_SAVE_MISSING'")
    add_test(NAME AstraCliRunNativeVNBadInput COMMAND powershell -NoProfile -ExecutionPolicy Bypass -Command "$out = & '$<TARGET_FILE:astra>' run '${CMAKE_BINARY_DIR}/Saved/Packages/NativeVN.astrapkg' --windowed-smoke --scripted-input '${CMAKE_SOURCE_DIR}/Samples/NativeVN/Input/qa/bad_input.yaml' --auto-close --json 2>&1; if ($LASTEXITCODE -eq 0) { $out; exit 1 }; $text = $out -join [Environment]::NewLine; if ($text -notmatch 'ASTRA_SCRIPTED_INPUT_PARSE_FAILED') { $out; exit 1 }; 'ASTRA_SCRIPTED_INPUT_PARSE_FAILED'")
    add_test(NAME AstraCliReplayNativeVN COMMAND astra replay "${CMAKE_BINARY_DIR}/Saved/Replays/NativeVNGolden.replay" --compare --json)
    set_tests_properties(AstraCliValidateNativeVNJson PROPERTIES PASS_REGULAR_EXPRESSION "phase8_script_vn")
    set_tests_properties(AstraCliValidateNativeVNLogs PROPERTIES PASS_REGULAR_EXPRESSION "phase8_script_vn")
    set_tests_properties(AstraCliRunPackageSmokeJson PROPERTIES PASS_REGULAR_EXPRESSION "media_release_gate")
    set_tests_properties(AstraCliPackageNativeVN PROPERTIES PASS_REGULAR_EXPRESSION "distribution_bundle")
    if(WIN32)
        set_tests_properties(AstraCliPackageNativeVNWin64Distribution PROPERTIES PASS_REGULAR_EXPRESSION "distribution-manifest.json")
        set_tests_properties(AstraCliPackageNativeVNLinuxDistributionMissing PROPERTIES PASS_REGULAR_EXPRESSION "ASTRA_DISTRIBUTION_TARGET_BINARIES_MISSING")
        set_tests_properties(AstraCliPackageNativeVNWin64Distribution PROPERTIES DEPENDS AstraCliPackageNativeVN)
    endif()
    set_tests_properties(AstraCliReleaseGateNativeVN PROPERTIES PASS_REGULAR_EXPRESSION "release.report")
    set_tests_properties(AstraCliRunNativeVNPackage PROPERTIES PASS_REGULAR_EXPRESSION "playable_vn")
    set_tests_properties(AstraCliRunNativeVNSave PROPERTIES PASS_REGULAR_EXPRESSION "save_snapshot_hash")
    set_tests_properties(AstraCliRunNativeVNLoad PROPERTIES PASS_REGULAR_EXPRESSION "save_slots")
    set_tests_properties(AstraCliRunNativeVNWindowedScripted PROPERTIES PASS_REGULAR_EXPRESSION "\"windowed_playable\"")
    set_tests_properties(AstraCliRunNativeVNQaTitleWalk AstraCliRunNativeVNQaSystemSaveLoad AstraCliRunNativeVNQaMenuReopen PROPERTIES PASS_REGULAR_EXPRESSION "\"player_qa\"")
    set_tests_properties(AstraCliRunNativeVNMissingSave PROPERTIES PASS_REGULAR_EXPRESSION "ASTRA_RUN_SAVE_MISSING")
    set_tests_properties(AstraCliRunNativeVNBadInput PROPERTIES PASS_REGULAR_EXPRESSION "ASTRA_SCRIPTED_INPUT_PARSE_FAILED")
    set_tests_properties(AstraCliReplayNativeVN PROPERTIES PASS_REGULAR_EXPRESSION "comparison")
    set_tests_properties(AstraCliReleaseGateNativeVN AstraCliRunNativeVNPackage AstraCliRunNativeVNSave AstraCliRunNativeVNWindowedScripted AstraCliRunNativeVNQaTitleWalk AstraCliRunNativeVNQaSystemSaveLoad AstraCliRunNativeVNQaMenuReopen AstraCliRunNativeVNMissingSave AstraCliRunNativeVNBadInput AstraCliReplayNativeVN PROPERTIES DEPENDS AstraCliPackageNativeVN)
    set_tests_properties(AstraCliRunNativeVNLoad PROPERTIES DEPENDS AstraCliRunNativeVNSave)
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
