cmake_minimum_required(VERSION 3.19)

add_subdirectory(minizip)

if(NOT EMSCRIPTEN AND NOT KRKR2_ASTRA_HOSTED)
    add_subdirectory(libbpg)
endif()
