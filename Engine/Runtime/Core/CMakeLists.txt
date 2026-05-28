astra_add_library(Astra_Core
    Private/Diagnostics.cpp
    Private/Log.cpp
    Private/Path.cpp
)

target_link_libraries(Astra_Core
    PUBLIC
        fmt::fmt
        spdlog::spdlog
)
