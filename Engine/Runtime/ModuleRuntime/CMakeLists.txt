set(ASTRA_MODULE_RUNTIME_SOURCES
    Private/ModuleRuntime.cpp
)

astra_add_library(Astra_ModuleRuntime ${ASTRA_MODULE_RUNTIME_SOURCES})
target_link_libraries(Astra_ModuleRuntime PUBLIC Astra_Core Astra_Platform yaml-cpp::yaml-cpp)
