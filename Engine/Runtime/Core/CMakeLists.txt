set(ASTRA_CORE_SOURCES
    Private/BuildInfo.cpp
    Private/Diagnostics.cpp
    Private/Error.cpp
    Private/Logging.cpp
    Private/Profiling.cpp
    Private/StableId.cpp
    Private/Config.cpp
    Private/Path.cpp
    Private/Serialization.cpp
    Private/Time.cpp
    "${CMAKE_BINARY_DIR}/Generated/AstraBuildInfo.cpp"
)

astra_add_library(AstraCore ${ASTRA_CORE_SOURCES})
target_link_libraries(AstraCore PUBLIC nlohmann_json::nlohmann_json)
target_compile_definitions(AstraCore PRIVATE ASTRA_BUILD_CONFIG="$<CONFIG>")
