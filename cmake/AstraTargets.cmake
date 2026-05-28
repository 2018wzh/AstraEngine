function(astra_configure_target target)
    target_compile_features(${target} PUBLIC cxx_std_23)
    if(MSVC)
        target_compile_options(${target} PRIVATE /W4 /permissive-)
    else()
        target_compile_options(${target} PRIVATE -Wall -Wextra -Wpedantic)
    endif()
endfunction()

function(astra_add_library target)
    add_library(${target} SHARED ${ARGN})
    set_target_properties(${target} PROPERTIES WINDOWS_EXPORT_ALL_SYMBOLS ON)
    astra_configure_target(${target})
    target_include_directories(${target}
        PUBLIC
            "${CMAKE_CURRENT_SOURCE_DIR}/Public"
        PRIVATE
            "${CMAKE_CURRENT_SOURCE_DIR}/Private"
    )
endfunction()

function(astra_add_header_only_library target)
    add_library(${target} INTERFACE)
    target_compile_features(${target} INTERFACE cxx_std_23)
    target_include_directories(${target}
        INTERFACE
            "${CMAKE_CURRENT_SOURCE_DIR}/Public"
    )
endfunction()
