set(ASTRA_PROPERTY_SYSTEM_SOURCES
    Private/PropertySystem.cpp
)

astra_add_library(AstraPropertySystem ${ASTRA_PROPERTY_SYSTEM_SOURCES})
target_link_libraries(AstraPropertySystem PUBLIC AstraCore)
