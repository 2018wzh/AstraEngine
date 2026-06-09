set(ASTRA_ASTRAVN_SOURCES
    Private/AstraVN.cpp
)

astra_add_library(Astra_AstraVN ${ASTRA_ASTRAVN_SOURCES})
target_link_libraries(Astra_AstraVN
    PUBLIC
        Astra_Core
        Astra_Asset
        Astra_Media
        Astra_Runtime
        Astra_Script
        nlohmann_json::nlohmann_json
)
