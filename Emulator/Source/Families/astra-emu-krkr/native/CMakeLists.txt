# Built from the pinned original core via KRKR2_EMBEDDED_ADAPTER_DIR.
if(NOT WINDOWS OR NOT MSVC)
    message(FATAL_ERROR "The restored NativeOwner currently requires Windows x64/MSVC")
endif()
add_library(astra_krkr_native SHARED NativeOwner.cpp NativeSession.cpp NativeInput.cpp)
target_include_directories(astra_krkr_native PRIVATE
    "$<TARGET_PROPERTY:krkr2,INCLUDE_DIRECTORIES>"
    "${CMAKE_CURRENT_SOURCE_DIR}/include")
target_compile_definitions(astra_krkr_native PRIVATE
    "$<TARGET_PROPERTY:krkr2,COMPILE_DEFINITIONS>")
target_compile_options(astra_krkr_native PRIVATE
    "$<TARGET_PROPERTY:krkr2,COMPILE_OPTIONS>")
target_link_libraries(astra_krkr_native PRIVATE
    "$<TARGET_PROPERTY:krkr2,LINK_LIBRARIES>")
target_link_options(astra_krkr_native PRIVATE
    "$<TARGET_PROPERTY:krkr2,LINK_OPTIONS>")
set_target_properties(astra_krkr_native PROPERTIES
    OUTPUT_NAME "astra_emu_krkr"
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_RUNTIME_OUTPUT_DIRECTORY}")
cocos_copy_target_dll(astra_krkr_native)
add_custom_command(TARGET astra_krkr_native POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E copy_directory
        "${GAME_RES_FOLDER}" "$<TARGET_FILE_DIR:astra_krkr_native>/Resources"
    VERBATIM)

