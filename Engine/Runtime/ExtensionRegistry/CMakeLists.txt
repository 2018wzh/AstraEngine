astra_add_library(Astra_ExtensionRegistry
    Private/ExtensionRegistry.cpp
)

target_link_libraries(Astra_ExtensionRegistry
    PUBLIC
        Astra_Core
)
