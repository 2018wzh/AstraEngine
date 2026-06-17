#include <Astra/Media/Media.hpp>

#include <utility>

namespace Astra::Media {

namespace {

Astra::Core::Diagnostic MakeDiagnostic(std::string code, Astra::Core::DiagnosticSeverity severity, std::string message) {
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = std::move(code);
    diagnostic.category = "media.decode";
    diagnostic.severity = severity;
    diagnostic.message = std::move(message);
    return diagnostic;
}

} // namespace

Astra::Core::Result<DecodedCpuBuffer> DecodeImageCpuBufferBytes(std::span<const Astra::Core::u8> bytes, Astra::Core::DiagnosticSink& diagnostics) {
    auto decoded = DecodeImageRgbaBytes(bytes, diagnostics);
    if (!decoded) {
        return Astra::Core::Result<DecodedCpuBuffer>::Failure(decoded.Error(), decoded.Message());
    }
    DecodedCpuBuffer buffer;
    buffer.width = decoded.Value().width;
    buffer.height = decoded.Value().height;
    buffer.row_stride = decoded.Value().width * 4;
    buffer.pixels = std::move(decoded.Value().pixels);
    return Astra::Core::Result<DecodedCpuBuffer>::Success(std::move(buffer));
}

Astra::Core::Result<VideoDecodeMetadata> InspectVideoBytes(std::span<const Astra::Core::u8> bytes, Astra::Core::DiagnosticSink& diagnostics) {
    if (bytes.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_DECODE_VIDEO_EMPTY", Astra::Core::DiagnosticSeverity::Blocking, "Video payload is empty."));
        return Astra::Core::Result<VideoDecodeMetadata>::Failure(Astra::Core::ErrorCode::InvalidArgument, "video payload empty");
    }
    diagnostics.Emit(MakeDiagnostic("ASTRA_DECODE_VIDEO_EXTENSION_POINT", Astra::Core::DiagnosticSeverity::Blocking, "FFmpeg video metadata/frame decode is a Phase 7 extension point and has no fake fallback."));
    return Astra::Core::Result<VideoDecodeMetadata>::Failure(Astra::Core::ErrorCode::Unsupported, "video decode extension point");
}

Astra::Core::Result<DecodedVideoFrame> DecodeVideoFrameRgbaBytes(std::span<const Astra::Core::u8> bytes, Astra::Core::u64, Astra::Core::DiagnosticSink& diagnostics) {
    if (bytes.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_DECODE_VIDEO_EMPTY", Astra::Core::DiagnosticSeverity::Blocking, "Video payload is empty."));
        return Astra::Core::Result<DecodedVideoFrame>::Failure(Astra::Core::ErrorCode::InvalidArgument, "video payload empty");
    }
    diagnostics.Emit(MakeDiagnostic("ASTRA_DECODE_VIDEO_EXTENSION_POINT", Astra::Core::DiagnosticSeverity::Blocking, "FFmpeg video frame decode is a Phase 7 extension point and has no fake fallback."));
    return Astra::Core::Result<DecodedVideoFrame>::Failure(Astra::Core::ErrorCode::Unsupported, "video frame decode extension point");
}

} // namespace Astra::Media
