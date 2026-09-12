cmake_minimum_required(VERSION 3.19)
project(core_base_module LANGUAGES CXX)

set(BASE_PATH ${CMAKE_CURRENT_LIST_DIR})
set(BASE_SOURCE_FILES
    ${BASE_PATH}/7zArchive.cpp
    ${BASE_PATH}/BinaryStream.cpp
    ${BASE_PATH}/CharacterSet.cpp
    ${BASE_PATH}/EventIntf.cpp
    ${BASE_PATH}/ScriptMgnIntf.cpp
    ${BASE_PATH}/StorageIntf.cpp
    ${BASE_PATH}/SysInitIntf.cpp
    ${BASE_PATH}/SystemIntf.cpp
    ${BASE_PATH}/TARArchive.cpp
    ${BASE_PATH}/TextStream.cpp
    ${BASE_PATH}/UtilStreams.cpp
    ${BASE_PATH}/XP3Archive.cpp
    ${BASE_PATH}/ZIPArchive.cpp
    ${BASE_PATH}/KAGParser.cpp
    ${BASE_PATH}/MsgIntf.cpp

    ${BASE_PATH}/impl/EventImpl.cpp
    ${BASE_PATH}/impl/FileSelector.cpp
    ${BASE_PATH}/impl/NativeEventQueue.cpp
    ${BASE_PATH}/impl/ScriptMgnImpl.cpp
    ${BASE_PATH}/impl/StorageImpl.cpp
    ${BASE_PATH}/impl/SysInitImpl.cpp
    ${BASE_PATH}/impl/SystemImpl.cpp
    ${BASE_PATH}/impl/MsgImpl.cpp
)

set(BASE_HEADERS_DIR
        ${BASE_PATH}/
        ${BASE_PATH}/impl
)

if(KRKR2_ASTRA_HOSTED)
    # The ZIP and 7z readers carry cocos/in-tree-7zip dependencies that the
    # hosted build does not ship; Kirikiri games mount XP3 archives.
    list(REMOVE_ITEM BASE_SOURCE_FILES ${BASE_PATH}/ZIPArchive.cpp)
    list(REMOVE_ITEM BASE_SOURCE_FILES ${BASE_PATH}/7zArchive.cpp)
endif()

add_library(${PROJECT_NAME} STATIC ${BASE_SOURCE_FILES})
target_include_directories(${PROJECT_NAME} PUBLIC ${BASE_HEADERS_DIR})
target_link_libraries(${PROJECT_NAME} PUBLIC tjs2 PRIVATE core_visual_module core_plugin_module core_environ_module core_extension_module core_sound_module core_utils_module)

find_package(7zip CONFIG REQUIRED)
find_package(LibArchive REQUIRED)
find_package(zstd CONFIG REQUIRED)
if(KRKR2_ASTRA_HOSTED)
    # Hosted build: the unrar port in this baseline publishes a DLL without
    # an import library; RAR archives are irrelevant to Kirikiri games, so
    # the unrar reader is compiled out (UtilStreams.cpp).
else()
    find_package(unrar CONFIG REQUIRED)
    find_package(cocos2dx CONFIG REQUIRED)
endif()

target_link_libraries(${PROJECT_NAME} PRIVATE
    7zip::7zip
    LibArchive::LibArchive
    zstd::libzstd
)
if(NOT KRKR2_ASTRA_HOSTED)
    target_link_libraries(${PROJECT_NAME} PRIVATE
        unrar::unrar
        cocos2dx::cocos2d
    )
endif()

if(EMSCRIPTEN)
    target_compile_options(${PROJECT_NAME} PRIVATE "SHELL:-s USE_SDL=2")
    target_link_options(${PROJECT_NAME} PUBLIC "SHELL:-s USE_SDL=2")
else()
    find_package(SDL2 CONFIG REQUIRED)
    target_link_libraries(${PROJECT_NAME} PRIVATE
        $<TARGET_NAME_IF_EXISTS:SDL2::SDL2main>
        $<IF:$<TARGET_EXISTS:SDL2::SDL2>,SDL2::SDL2,SDL2::SDL2-static>
    )
endif()
