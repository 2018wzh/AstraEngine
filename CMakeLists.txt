cmake_minimum_required(VERSION 3.28)

project(AstraEngine VERSION 0.2.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/Bin")
set(CMAKE_LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/Lib")
set(CMAKE_ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/Lib")

option(ASTRA_BUILD_TESTS "Build AstraEngine tests" ON)

set(ASTRA_PLUGIN_OUTPUT_ROOT "${CMAKE_BINARY_DIR}/Plugins")

include(CTest)
include(cmake/AstraTargets.cmake)

find_package(SDL3 CONFIG REQUIRED)
find_package(fmt CONFIG REQUIRED)
find_package(spdlog CONFIG REQUIRED)
find_package(nlohmann_json CONFIG REQUIRED)
find_package(yaml-cpp CONFIG REQUIRED)

if(ASTRA_BUILD_TESTS)
    find_package(Catch2 CONFIG REQUIRED)
endif()

add_subdirectory(Engine/Runtime/Core)
add_subdirectory(Engine/Runtime/Platform)
add_subdirectory(Engine/Runtime/ModuleRuntime)
add_subdirectory(Engine/Runtime/PropertySystem)
add_subdirectory(Engine/Plugins/Examples/Phase1ExampleModule)
add_subdirectory(Engine/Programs/AstraPhase1Smoke)

if(ASTRA_BUILD_TESTS)
    add_subdirectory(Engine/Tests)
endif()
