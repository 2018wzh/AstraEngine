astra_add_library(Astra_VNRuntimeServices
    Private/RuntimeServices.cpp
)

target_link_libraries(Astra_VNRuntimeServices
    PUBLIC
        Astra_AssetCore
        Astra_Core
        nlohmann_json::nlohmann_json
    PRIVATE
        EnTT::EnTT
)
