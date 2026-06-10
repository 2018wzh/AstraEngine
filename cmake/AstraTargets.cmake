include(GenerateExportHeader)

function(astra_configure_target target)
    target_compile_features(${target} PUBLIC cxx_std_23)
    if(MSVC)
        target_compile_options(${target} PRIVATE /W4 /permissive-)
    else()
        target_compile_options(${target} PRIVATE -Wall -Wextra -Wpedantic)
    endif()
endfunction()

function(astra_target_export_info target out_module out_macro)
    if(target STREQUAL "AstraCore")
        set(module "Core")
        set(macro "ASTRA_CORE_API")
    elseif(target STREQUAL "AstraPlatform")
        set(module "Platform")
        set(macro "ASTRA_PLATFORM_API")
    elseif(target STREQUAL "AstraModuleRuntime")
        set(module "ModuleRuntime")
        set(macro "ASTRA_MODULE_RUNTIME_API")
    elseif(target STREQUAL "AstraPropertySystem")
        set(module "PropertySystem")
        set(macro "ASTRA_PROPERTY_SYSTEM_API")
    elseif(target STREQUAL "AstraScene")
        set(module "Scene")
        set(macro "ASTRA_SCENE_API")
    elseif(target STREQUAL "AstraRuntime")
        set(module "Runtime")
        set(macro "ASTRA_RUNTIME_API")
    elseif(target STREQUAL "AstraAsset")
        set(module "Asset")
        set(macro "ASTRA_ASSET_API")
    elseif(target STREQUAL "AstraMedia")
        set(module "Media")
        set(macro "ASTRA_MEDIA_API")
    elseif(target STREQUAL "AstraScript")
        set(module "Script")
        set(macro "ASTRA_SCRIPT_API")
    elseif(target STREQUAL "AstraVN")
        set(module "AstraVN")
        set(macro "ASTRA_ASTRAVN_API")
    elseif(target STREQUAL "AstraTools")
        set(module "Tools")
        set(macro "ASTRA_TOOLS_API")
    else()
        string(REGEX REPLACE "^Astra" "" module "${target}")
        string(TOUPPER "${target}" macro)
        string(REPLACE "_" "_" macro "${macro}")
        set(macro "${macro}_API")
    endif()
    set(${out_module} "${module}" PARENT_SCOPE)
    set(${out_macro} "${macro}" PARENT_SCOPE)
endfunction()

function(astra_add_library target)
    add_library(${target} SHARED ${ARGN})
    astra_configure_target(${target})
    astra_target_export_info(${target} ASTRA_EXPORT_MODULE ASTRA_EXPORT_MACRO)
    set(ASTRA_EXPORT_HEADER "${CMAKE_CURRENT_BINARY_DIR}/Generated/Astra/${ASTRA_EXPORT_MODULE}/Export.hpp")
    generate_export_header(${target}
        EXPORT_FILE_NAME "${ASTRA_EXPORT_HEADER}"
        EXPORT_MACRO_NAME "${ASTRA_EXPORT_MACRO}"
        NO_EXPORT_MACRO_NAME "${ASTRA_EXPORT_MACRO}_NO_EXPORT"
        DEPRECATED_MACRO_NAME "${ASTRA_EXPORT_MACRO}_DEPRECATED"
    )
    set_target_properties(${target} PROPERTIES
        WINDOWS_EXPORT_ALL_SYMBOLS ON
        RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/Bin"
        LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/Bin"
        ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/Lib"
    )
    target_include_directories(${target}
        PUBLIC
            "${CMAKE_CURRENT_SOURCE_DIR}/Public"
            "${CMAKE_CURRENT_BINARY_DIR}/Generated"
        PRIVATE
            "${CMAKE_CURRENT_SOURCE_DIR}/Private"
    )
endfunction()

function(astra_add_module target)
    add_library(${target} MODULE ${ARGN})
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
