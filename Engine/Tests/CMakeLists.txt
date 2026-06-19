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
        AstraGame
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
    set_tests_properties(AstraCliDocCheck PROPERTIES WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}")
    add_test(NAME AstraCliValidatePackageLaunch COMMAND astra validate "${CMAKE_SOURCE_DIR}/Samples/PackageLaunch" --strict)
    add_test(NAME AstraCliValidateNativeVN COMMAND astra validate "${CMAKE_SOURCE_DIR}/Samples/NativeVN" --strict)
    add_test(NAME AstraCliValidateNativeVNJson COMMAND astra validate "${CMAKE_SOURCE_DIR}/Samples/NativeVN" --strict --json)
    add_test(NAME AstraCliValidateNativeVNLogs COMMAND astra validate "${CMAKE_SOURCE_DIR}/Samples/NativeVN" --strict --json --log-dir "${CMAKE_BINARY_DIR}/Saved/TestLogs" --log-level trace)
    add_test(NAME AstraCliPackageLaunch COMMAND astra package "${CMAKE_SOURCE_DIR}/Samples/PackageLaunch" --profile development)
    add_test(NAME AstraCliPackageNativeVN COMMAND astra package "${CMAKE_SOURCE_DIR}/Samples/NativeVN" --profile deterministic --json)
    if(WIN32)
        add_test(NAME AstraCliPackageNativeVNWin64Distribution COMMAND powershell -NoProfile -ExecutionPolicy Bypass -Command "$root = '${CMAKE_BINARY_DIR}/Saved/Releases/NativeVN/NativeVN-win64'; $required = @('NativeVN.exe','distribution-manifest.json','Packages/NativeVN.astrapkg','Plugins/Phase1Example/Phase1Example.plugin.yaml','Plugins/Phase1Example/Bin/win64/AstraExampleFoundationPlugin.dll'); foreach ($item in $required) { if (!(Test-Path (Join-Path $root $item))) { Write-Output \"missing $item\"; exit 1 } }; 'distribution-manifest.json'")
        add_test(NAME AstraCliPackageNativeVNLinuxDistributionMissing COMMAND powershell -NoProfile -ExecutionPolicy Bypass -Command "$out = & '$<TARGET_FILE:astra>' package '${CMAKE_SOURCE_DIR}/Samples/NativeVN' --profile deterministic --target-platform linux-x64 --json 2>&1; if ($LASTEXITCODE -eq 0) { $out; exit 1 }; $text = $out -join [Environment]::NewLine; if ($text -notmatch 'ASTRA_DISTRIBUTION_TARGET_BINARIES_MISSING') { $out; exit 1 }; 'ASTRA_DISTRIBUTION_TARGET_BINARIES_MISSING'")
    endif()
    add_test(NAME AstraCliReleaseGateNativeVN COMMAND astra release-gate "${CMAKE_SOURCE_DIR}/Samples/NativeVN" --profile deterministic --json)
    add_test(NAME AstraGameLaunchNativeVN COMMAND "${CMAKE_BINARY_DIR}/Saved/Releases/NativeVN/NativeVN-win64/NativeVN.exe" --backend headless --json)
    set_tests_properties(AstraCliValidateNativeVNJson PROPERTIES PASS_REGULAR_EXPRESSION "phase8_script_vn")
    set_tests_properties(AstraCliValidateNativeVNLogs PROPERTIES PASS_REGULAR_EXPRESSION "phase8_script_vn")
    set_tests_properties(AstraCliPackageNativeVN PROPERTIES PASS_REGULAR_EXPRESSION "distribution_bundle")
    if(WIN32)
        set_tests_properties(AstraCliPackageNativeVNWin64Distribution PROPERTIES PASS_REGULAR_EXPRESSION "distribution-manifest.json")
        set_tests_properties(AstraCliPackageNativeVNLinuxDistributionMissing PROPERTIES PASS_REGULAR_EXPRESSION "ASTRA_DISTRIBUTION_TARGET_BINARIES_MISSING")
        set_tests_properties(AstraCliPackageNativeVNWin64Distribution PROPERTIES DEPENDS AstraCliPackageNativeVN)
    endif()
    set_tests_properties(AstraCliReleaseGateNativeVN PROPERTIES PASS_REGULAR_EXPRESSION "release.report")
    set_tests_properties(AstraGameLaunchNativeVN PROPERTIES PASS_REGULAR_EXPRESSION "astra.game.run_report.v1")
    set_tests_properties(AstraCliReleaseGateNativeVN AstraGameLaunchNativeVN PROPERTIES DEPENDS AstraCliPackageNativeVN)
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
