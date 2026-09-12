cmake_minimum_required(VERSION 3.15)
project(minizip)

add_library(minizip STATIC
    zip.c
    unzip/unzip.c
    unzip/ioapi.c
)
# The engine never writes password-protected ZIP entries; compile the
# ZipCrypto path out instead of sourcing a matching crypt.h generation.
target_compile_definitions(${PROJECT_NAME} PRIVATE NOCRYPT)
install(DIRECTORY "${CMAKE_CURRENT_LIST_DIR}/"
        DESTINATION "include/minizip"
        FILES_MATCHING PATTERN "*.h"
)

target_include_directories(${PROJECT_NAME}
        PUBLIC
            $<BUILD_INTERFACE:${CMAKE_CURRENT_LIST_DIR}/..>
            $<BUILD_INTERFACE:${CMAKE_CURRENT_LIST_DIR}>
            $<BUILD_INTERFACE:${CMAKE_CURRENT_LIST_DIR}/unzip>
        INTERFACE
            $<INSTALL_INTERFACE:include/minizip>
)

find_package(ZLIB REQUIRED)
target_link_libraries(${PROJECT_NAME} PUBLIC ZLIB::ZLIB)
