astra_add_library(Astra_AudioCore
    Private/AudioPlugin.cpp
    Private/AudioEngine.cpp
)

target_link_libraries(Astra_AudioCore
    PUBLIC
        Astra_Bootstrap
        Astra_Core
)

target_include_directories(Astra_AudioCore PRIVATE ${MINIAUDIO_INCLUDE_DIRS})
