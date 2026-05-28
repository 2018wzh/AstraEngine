astra_add_library(Astra_VFS
    Private/DirectoryMount.cpp
)

target_link_libraries(Astra_VFS
    PUBLIC
        Astra_Core
)
