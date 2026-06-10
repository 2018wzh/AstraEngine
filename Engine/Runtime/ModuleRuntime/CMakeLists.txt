set(ASTRA_MODULE_RUNTIME_SOURCES
    Private/ModuleRuntime.cpp
)

astra_add_library(AstraModuleRuntime ${ASTRA_MODULE_RUNTIME_SOURCES})
target_link_libraries(AstraModuleRuntime PUBLIC AstraCore AstraPlatform yaml-cpp::yaml-cpp)
