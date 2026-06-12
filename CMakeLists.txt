cmake_minimum_required(VERSION 3.28)

project(AstraEngine VERSION 0.2.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/Bin")
set(CMAKE_LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/Lib")
set(CMAKE_ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/Lib")
set(CMAKE_PDB_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/Symbols")

option(ASTRA_BUILD_RUNTIME "Build AstraEngine runtime foundation libraries" ON)
option(ASTRA_BUILD_TOOLS "Build AstraEngine command line tools" ON)
option(ASTRA_BUILD_SAMPLES "Build AstraEngine sample projects and plugins" ON)
option(ASTRA_BUILD_PLUGINS "Build AstraEngine plugins" ON)
option(ASTRA_BUILD_TESTS "Build AstraEngine tests" ON)
option(ASTRA_BUILD_DOC_CHECK "Enable Phase 0 documentation validation target" ON)
option(ASTRA_BUILD_EDITOR "Build AstraEngine editor tools" OFF)
option(ASTRA_ENABLE_SDL_BACKEND "Build SDL-backed platform backend" ON)
option(ASTRA_ENABLE_HEADLESS_BACKEND "Build headless platform backend" ON)

set(ASTRA_PLUGIN_OUTPUT_ROOT "${CMAKE_BINARY_DIR}/Plugins")
set(ASTRA_ABI_VERSION 1)

if(NOT CMAKE_CONFIGURATION_TYPES AND NOT CMAKE_BUILD_TYPE)
    set(CMAKE_BUILD_TYPE Debug CACHE STRING "Build type" FORCE)
endif()

set_property(GLOBAL PROPERTY USE_FOLDERS ON)

include(CTest)
include(cmake/AstraTargets.cmake)

find_package(nlohmann_json CONFIG REQUIRED)
find_package(yaml-cpp CONFIG REQUIRED)
find_package(sol2 CONFIG REQUIRED)
find_package(unofficial-lua CONFIG REQUIRED)
find_package(zstd CONFIG REQUIRED)

if(ASTRA_BUILD_TOOLS)
    find_package(CLI11 CONFIG REQUIRED)
    find_package(OpenSSL REQUIRED)
endif()

if(ASTRA_ENABLE_SDL_BACKEND)
    find_package(SDL3 CONFIG REQUIRED)
endif()

if(ASTRA_BUILD_TESTS)
    find_package(Catch2 CONFIG REQUIRED)
endif()

execute_process(
    COMMAND git rev-parse --short HEAD
    WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
    OUTPUT_VARIABLE ASTRA_GIT_COMMIT
    OUTPUT_STRIP_TRAILING_WHITESPACE
    ERROR_QUIET
)
if(NOT ASTRA_GIT_COMMIT)
    set(ASTRA_GIT_COMMIT "unknown")
endif()

foreach(ASTRA_BOOL_NAME
    ASTRA_BUILD_RUNTIME
    ASTRA_BUILD_TOOLS
    ASTRA_BUILD_TESTS
    ASTRA_BUILD_SAMPLES
    ASTRA_BUILD_PLUGINS
    ASTRA_BUILD_EDITOR
    ASTRA_ENABLE_HEADLESS_BACKEND
    ASTRA_ENABLE_SDL_BACKEND
)
    if(${ASTRA_BOOL_NAME})
        set(${ASTRA_BOOL_NAME}_VALUE 1)
    else()
        set(${ASTRA_BOOL_NAME}_VALUE 0)
    endif()
endforeach()

configure_file(
    "${CMAKE_CURRENT_SOURCE_DIR}/Engine/Runtime/Core/Private/BuildInfo.generated.cpp.in"
    "${CMAKE_CURRENT_BINARY_DIR}/Generated/AstraBuildInfo.cpp"
    @ONLY
)

if(ASTRA_BUILD_RUNTIME OR ASTRA_BUILD_TOOLS)
    add_subdirectory(Engine/Runtime/Core)
    add_subdirectory(Engine/Runtime/Platform)
    add_subdirectory(Engine/Runtime/ModuleRuntime)
    add_subdirectory(Engine/Runtime/Asset)
    add_subdirectory(Engine/Runtime/Media)
endif()

if(ASTRA_BUILD_RUNTIME)
    find_package(EnTT CONFIG REQUIRED)
    add_subdirectory(Engine/Runtime/PropertySystem)
    add_subdirectory(Engine/Runtime/Scene)
    add_subdirectory(Engine/Runtime/Runtime)
    add_subdirectory(Engine/Runtime/Script)
    add_subdirectory(Engine/Runtime/AstraVN)
endif()

if(ASTRA_BUILD_TOOLS)
    add_subdirectory(Engine/Programs/astra)
endif()

if(ASTRA_BUILD_PLUGINS AND ASTRA_BUILD_SAMPLES)
    add_subdirectory(Engine/Plugins/Examples/Phase1Example)
endif()

if(ASTRA_BUILD_TESTS AND BUILD_TESTING)
    add_subdirectory(Engine/Tests)
endif()

if(ASTRA_BUILD_DOC_CHECK)
    find_program(ASTRA_POWERSHELL_EXECUTABLE
        NAMES pwsh powershell
        REQUIRED
    )

    add_custom_target(AstraDocCheck
        COMMAND
            "${ASTRA_POWERSHELL_EXECUTABLE}"
            -NoProfile
            -ExecutionPolicy
            Bypass
            -File
            "${CMAKE_CURRENT_SOURCE_DIR}/tools/doc-check.ps1"
            -Root
            "${CMAKE_CURRENT_SOURCE_DIR}"
        WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
        COMMENT "Running AstraEngine Phase 0 documentation checks"
        VERBATIM
    )

    if(BUILD_TESTING)
        add_test(
            NAME AstraDocCheck
            COMMAND
                "${ASTRA_POWERSHELL_EXECUTABLE}"
                -NoProfile
                -ExecutionPolicy
                Bypass
                -File
                "${CMAKE_CURRENT_SOURCE_DIR}/tools/doc-check.ps1"
                -Root
                "${CMAKE_CURRENT_SOURCE_DIR}"
        )
    endif()
endif()
