#include <Astra/Media/Media.hpp>

#if defined(ASTRA_ENABLE_SDL_BACKEND)
#include <SDL3/SDL.h>
#endif

namespace Astra::Media {

std::unique_ptr<IRenderer2D> CreateSdlRenderer2DStub(Astra::Core::DiagnosticSink& diagnostics) {
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = "ASTRA_MEDIA_SDL_STUB";
    diagnostic.category = "media.foundation";
    diagnostic.severity = Astra::Core::DiagnosticSeverity::Info;
    diagnostic.message = "SDL renderer factory is a Phase 3 foundation compile-path stub; using headless renderer.";
    diagnostics.Emit(std::move(diagnostic));
#if defined(ASTRA_ENABLE_SDL_BACKEND)
    (void)SDL_GetVersion();
#endif
    return CreateHeadlessRenderer2D();
}

} // namespace Astra::Media
