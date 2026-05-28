astra_add_library(Astra_Renderer2D
    Private/SdlGpuRendererPlugin.cpp
    Private/SdlGpuRenderer.cpp
)

target_link_libraries(Astra_Renderer2D
    PUBLIC
        Astra_Bootstrap
        Astra_Core
        Astra_RHI
        Astra_VNRuntimeServices
        SDL3::SDL3
)
