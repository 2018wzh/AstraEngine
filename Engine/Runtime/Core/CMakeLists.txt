astra_add_library(Astra_Core
    Private/Assert.cpp
    Private/Config.cpp
    Private/Diagnostics.cpp
    Private/Log.cpp
    Private/Path.cpp
    Private/Time.cpp
)

target_link_libraries(Astra_Core
    PUBLIC
        fmt::fmt
        nlohmann_json::nlohmann_json
        spdlog::spdlog
        yaml-cpp::yaml-cpp
)
