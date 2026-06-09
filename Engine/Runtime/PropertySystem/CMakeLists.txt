set(ASTRA_PROPERTY_SYSTEM_SOURCES
    Private/PropertySystem.cpp
)

astra_add_library(Astra_PropertySystem ${ASTRA_PROPERTY_SYSTEM_SOURCES})
target_link_libraries(Astra_PropertySystem PUBLIC Astra_Core)
