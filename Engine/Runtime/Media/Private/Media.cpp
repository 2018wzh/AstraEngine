#include <Astra/Media/Media.hpp>

#if defined(ASTRA_MEDIA_HAS_LIBPNG)
#include <png.h>
#endif
#if defined(ASTRA_MEDIA_HAS_LIBJPEG)
#include <jpeglib.h>
#endif
#if defined(ASTRA_MEDIA_HAS_LIBWEBP)
#include <webp/decode.h>
#endif
#if defined(ASTRA_MEDIA_HAS_FREETYPE)
#include <ft2build.h>
#include FT_FREETYPE_H
#endif
#if defined(ASTRA_MEDIA_HAS_HARFBUZZ)
#include <hb.h>
#include <hb-ft.h>
#endif
#if defined(ASTRA_ENABLE_SDL_BACKEND)
#include <SDL3/SDL_version.h>
#endif
#if defined(ASTRA_MEDIA_HAS_MINIAUDIO)
#define MINIAUDIO_IMPLEMENTATION
#include <miniaudio.h>
#ifdef min
#undef min
#endif
#ifdef max
#undef max
#endif
#endif
#if defined(ASTRA_MEDIA_HAS_VORBISFILE)
#include <vorbis/vorbisfile.h>
#endif
#if defined(ASTRA_MEDIA_HAS_FFMPEG)
extern "C" {
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/avutil.h>
#include <libavutil/error.h>
}
#endif

#include <algorithm>
#include <array>
#include <csetjmp>
#include <cstring>
#include <initializer_list>
#include <sstream>

namespace Astra::Media {

namespace {

Astra::Core::Diagnostic MakeDiagnostic(std::string code, Astra::Core::DiagnosticSeverity severity, std::string message) {
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = std::move(code);
    diagnostic.category = "media.foundation";
    diagnostic.severity = severity;
    diagnostic.message = std::move(message);
    return diagnostic;
}

std::string StableHash(std::string_view text) {
    constexpr Astra::Core::u64 offset = 14695981039346656037ull;
    constexpr Astra::Core::u64 prime = 1099511628211ull;
    Astra::Core::u64 value = offset;
    for (const auto character : text) {
        value ^= static_cast<unsigned char>(character);
        value *= prime;
    }
    std::ostringstream output;
    output << std::hex << value;
    return output.str();
}

bool IsRequiredMediaSlot(std::string_view slot_id) {
    return slot_id == Renderer2DSlotId || slot_id == TextLayoutSlotId || slot_id == AudioSlotId;
}

void EmitBlocking(Astra::Core::DiagnosticSink& diagnostics, std::string code, std::string message, std::string object_id = {}) {
    auto diagnostic = MakeDiagnostic(std::move(code), Astra::Core::DiagnosticSeverity::Blocking, std::move(message));
    if (!object_id.empty()) {
        diagnostic.objects.push_back({"media_provider", std::move(object_id)});
    }
    diagnostics.Emit(std::move(diagnostic));
}

void AddUnique(std::vector<std::string>& values, std::string value) {
    if (std::ranges::find(values, value) == values.end()) {
        values.push_back(std::move(value));
    }
}

[[maybe_unused]] std::vector<Astra::Core::u32> DecodeUtf8Codepoints(std::string_view text) {
    std::vector<Astra::Core::u32> codepoints;
    for (std::size_t index = 0; index < text.size();) {
        const auto byte = static_cast<unsigned char>(text[index]);
        if (byte < 0x80) {
            codepoints.push_back(byte);
            ++index;
        } else if ((byte & 0xe0) == 0xc0 && index + 1 < text.size()) {
            codepoints.push_back(((byte & 0x1f) << 6) | (static_cast<unsigned char>(text[index + 1]) & 0x3f));
            index += 2;
        } else if ((byte & 0xf0) == 0xe0 && index + 2 < text.size()) {
            codepoints.push_back(((byte & 0x0f) << 12) | ((static_cast<unsigned char>(text[index + 1]) & 0x3f) << 6) | (static_cast<unsigned char>(text[index + 2]) & 0x3f));
            index += 3;
        } else if ((byte & 0xf8) == 0xf0 && index + 3 < text.size()) {
            codepoints.push_back(((byte & 0x07) << 18) | ((static_cast<unsigned char>(text[index + 1]) & 0x3f) << 12) | ((static_cast<unsigned char>(text[index + 2]) & 0x3f) << 6) | (static_cast<unsigned char>(text[index + 3]) & 0x3f));
            index += 4;
        } else {
            codepoints.push_back('?');
            ++index;
        }
    }
    return codepoints;
}

std::string VersionFromPackedInteger(int version) {
    const auto major = (version >> 16) & 0xff;
    const auto minor = (version >> 8) & 0xff;
    const auto patch = version & 0xff;
    return std::to_string(major) + "." + std::to_string(minor) + "." + std::to_string(patch);
}

#if defined(ASTRA_MEDIA_HAS_LIBPNG)
struct PngMemoryReader {
    std::span<const Astra::Core::u8> bytes;
    std::size_t offset = 0;
};

void PngReadCallback(png_structp png, png_bytep output, png_size_t size) {
    auto* reader = static_cast<PngMemoryReader*>(png_get_io_ptr(png));
    if (reader == nullptr || reader->offset + size > reader->bytes.size()) {
        png_error(png, "PNG input ended unexpectedly");
        return;
    }
    std::memcpy(output, reader->bytes.data() + reader->offset, size);
    reader->offset += size;
}
#endif

#if defined(ASTRA_MEDIA_HAS_FFMPEG)
struct FfmpegMemoryReader {
    std::span<const Astra::Core::u8> bytes;
    std::size_t offset = 0;
};

std::string FfmpegErrorString(int error) {
    std::array<char, AV_ERROR_MAX_STRING_SIZE> buffer{};
    if (av_strerror(error, buffer.data(), buffer.size()) == 0) {
        return buffer.data();
    }
    return "unknown ffmpeg error";
}

int FfmpegReadPacket(void* opaque, std::uint8_t* output, int output_size) {
    auto* reader = static_cast<FfmpegMemoryReader*>(opaque);
    if (reader == nullptr || output == nullptr || output_size <= 0 || reader->offset >= reader->bytes.size()) {
        return AVERROR_EOF;
    }
    const auto remaining = reader->bytes.size() - reader->offset;
    const auto bytes_to_read = std::min<std::size_t>(remaining, static_cast<std::size_t>(output_size));
    std::memcpy(output, reader->bytes.data() + reader->offset, bytes_to_read);
    reader->offset += bytes_to_read;
    return static_cast<int>(bytes_to_read);
}

std::int64_t FfmpegSeek(void* opaque, std::int64_t offset, int whence) {
    auto* reader = static_cast<FfmpegMemoryReader*>(opaque);
    if (reader == nullptr) {
        return -1;
    }
    if (whence == AVSEEK_SIZE) {
        return static_cast<std::int64_t>(reader->bytes.size());
    }

    const auto base_whence = whence & ~AVSEEK_FORCE;
    std::int64_t next = 0;
    if (base_whence == SEEK_SET) {
        next = offset;
    } else if (base_whence == SEEK_CUR) {
        next = static_cast<std::int64_t>(reader->offset) + offset;
    } else if (base_whence == SEEK_END) {
        next = static_cast<std::int64_t>(reader->bytes.size()) + offset;
    } else {
        return -1;
    }

    if (next < 0 || static_cast<std::uint64_t>(next) > reader->bytes.size()) {
        return -1;
    }
    reader->offset = static_cast<std::size_t>(next);
    return next;
}

Astra::Core::Result<AudioDecodeReport> DecodeAudioBytesWithFfmpeg(std::span<const Astra::Core::u8> bytes, Astra::Core::DiagnosticSink& diagnostics) {
    av_log_set_level(AV_LOG_QUIET);

    FfmpegMemoryReader reader{bytes, 0};
    constexpr int kAvioBufferSize = 4096;
    auto* avio_buffer = static_cast<unsigned char*>(av_malloc(kAvioBufferSize));
    if (avio_buffer == nullptr) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_FFMPEG_ALLOC_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "FFmpeg could not allocate an input buffer."));
        return Astra::Core::Result<AudioDecodeReport>::Failure(Astra::Core::ErrorCode::InternalError, "ffmpeg avio allocation failed");
    }

    AVIOContext* avio = avio_alloc_context(avio_buffer, kAvioBufferSize, 0, &reader, &FfmpegReadPacket, nullptr, &FfmpegSeek);
    if (avio == nullptr) {
        av_free(avio_buffer);
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_FFMPEG_ALLOC_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "FFmpeg could not allocate an AVIO context."));
        return Astra::Core::Result<AudioDecodeReport>::Failure(Astra::Core::ErrorCode::InternalError, "ffmpeg avio context allocation failed");
    }

    AVFormatContext* format_context = avformat_alloc_context();
    if (format_context == nullptr) {
        avio_context_free(&avio);
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_FFMPEG_ALLOC_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "FFmpeg could not allocate a format context."));
        return Astra::Core::Result<AudioDecodeReport>::Failure(Astra::Core::ErrorCode::InternalError, "ffmpeg format allocation failed");
    }
    format_context->pb = avio;
    format_context->flags |= AVFMT_FLAG_CUSTOM_IO;

    const bool is_ogg = bytes.size() >= 4 && bytes[0] == 'O' && bytes[1] == 'g' && bytes[2] == 'g' && bytes[3] == 'S';
    const AVInputFormat* input_format = is_ogg ? av_find_input_format("ogg") : nullptr;
    int result = avformat_open_input(&format_context, nullptr, input_format, nullptr);
    if (result < 0) {
        avformat_free_context(format_context);
        avio_context_free(&avio);
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_AUDIO_DECODE_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "FFmpeg could not open the audio payload: " + FfmpegErrorString(result)));
        return Astra::Core::Result<AudioDecodeReport>::Failure(Astra::Core::ErrorCode::InvalidFormat, "ffmpeg open failed");
    }

    result = avformat_find_stream_info(format_context, nullptr);
    if (result < 0) {
        avformat_close_input(&format_context);
        avio_context_free(&avio);
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_AUDIO_DECODE_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "FFmpeg could not read stream info: " + FfmpegErrorString(result)));
        return Astra::Core::Result<AudioDecodeReport>::Failure(Astra::Core::ErrorCode::InvalidFormat, "ffmpeg stream info failed");
    }

    const int stream_index = av_find_best_stream(format_context, AVMEDIA_TYPE_AUDIO, -1, -1, nullptr, 0);
    if (stream_index < 0) {
        avformat_close_input(&format_context);
        avio_context_free(&avio);
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_AUDIO_STREAM_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "FFmpeg did not find an audio stream in the payload."));
        return Astra::Core::Result<AudioDecodeReport>::Failure(Astra::Core::ErrorCode::InvalidFormat, "ffmpeg audio stream missing");
    }

    AVStream* stream = format_context->streams[stream_index];
    const AVCodec* codec = avcodec_find_decoder(stream->codecpar->codec_id);
    if (codec == nullptr) {
        avformat_close_input(&format_context);
        avio_context_free(&avio);
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_AUDIO_CODEC_UNSUPPORTED", Astra::Core::DiagnosticSeverity::Blocking, "FFmpeg found no decoder for the audio stream."));
        return Astra::Core::Result<AudioDecodeReport>::Failure(Astra::Core::ErrorCode::Unsupported, "ffmpeg codec missing");
    }

    AVCodecContext* codec_context = avcodec_alloc_context3(codec);
    if (codec_context == nullptr) {
        avformat_close_input(&format_context);
        avio_context_free(&avio);
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_FFMPEG_ALLOC_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "FFmpeg could not allocate a codec context."));
        return Astra::Core::Result<AudioDecodeReport>::Failure(Astra::Core::ErrorCode::InternalError, "ffmpeg codec allocation failed");
    }

    result = avcodec_parameters_to_context(codec_context, stream->codecpar);
    if (result >= 0) {
        result = avcodec_open2(codec_context, codec, nullptr);
    }
    if (result < 0) {
        avcodec_free_context(&codec_context);
        avformat_close_input(&format_context);
        avio_context_free(&avio);
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_AUDIO_DECODE_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "FFmpeg could not open the audio decoder: " + FfmpegErrorString(result)));
        return Astra::Core::Result<AudioDecodeReport>::Failure(Astra::Core::ErrorCode::InvalidFormat, "ffmpeg codec open failed");
    }

    AVPacket* packet = av_packet_alloc();
    AVFrame* frame = av_frame_alloc();
    if (packet == nullptr || frame == nullptr) {
        av_packet_free(&packet);
        av_frame_free(&frame);
        avcodec_free_context(&codec_context);
        avformat_close_input(&format_context);
        avio_context_free(&avio);
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_FFMPEG_ALLOC_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "FFmpeg could not allocate decode packet/frame state."));
        return Astra::Core::Result<AudioDecodeReport>::Failure(Astra::Core::ErrorCode::InternalError, "ffmpeg packet/frame allocation failed");
    }

    Astra::Core::u64 frame_count = 0;
    while (av_read_frame(format_context, packet) >= 0) {
        if (packet->stream_index == stream_index && avcodec_send_packet(codec_context, packet) >= 0) {
            while (avcodec_receive_frame(codec_context, frame) == 0) {
                frame_count += static_cast<Astra::Core::u64>(std::max(0, frame->nb_samples));
                av_frame_unref(frame);
            }
        }
        av_packet_unref(packet);
    }
    if (avcodec_send_packet(codec_context, nullptr) >= 0) {
        while (avcodec_receive_frame(codec_context, frame) == 0) {
            frame_count += static_cast<Astra::Core::u64>(std::max(0, frame->nb_samples));
            av_frame_unref(frame);
        }
    }

    AudioDecodeReport report;
    report.format = format_context->iformat == nullptr || format_context->iformat->name == nullptr ? "container" : format_context->iformat->name;
    report.channels = static_cast<Astra::Core::u32>(std::max(0, codec_context->ch_layout.nb_channels));
    report.sample_rate = static_cast<Astra::Core::u32>(std::max(0, codec_context->sample_rate));
    report.pcm_frame_count = frame_count;
    report.decoded_by = std::string("ffmpeg/") + (codec->name == nullptr ? "audio" : codec->name);

    av_packet_free(&packet);
    av_frame_free(&frame);
    avcodec_free_context(&codec_context);
    avformat_close_input(&format_context);
    avio_context_free(&avio);

    if (report.pcm_frame_count == 0 || report.channels == 0 || report.sample_rate == 0) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_AUDIO_DECODE_EMPTY", Astra::Core::DiagnosticSeverity::Blocking, "FFmpeg audio decode produced no PCM frames."));
        return Astra::Core::Result<AudioDecodeReport>::Failure(Astra::Core::ErrorCode::InvalidFormat, "ffmpeg decode empty");
    }
    return Astra::Core::Result<AudioDecodeReport>::Success(std::move(report));
}
#endif

#if defined(ASTRA_MEDIA_HAS_VORBISFILE)
struct VorbisMemoryReader {
    std::span<const Astra::Core::u8> bytes;
    std::size_t offset = 0;
};

std::size_t VorbisReadCallback(void* ptr, std::size_t size, std::size_t nmemb, void* datasource) {
    auto* reader = static_cast<VorbisMemoryReader*>(datasource);
    if (reader == nullptr || size == 0 || nmemb == 0 || reader->offset >= reader->bytes.size()) {
        return 0;
    }
    const auto remaining = reader->bytes.size() - reader->offset;
    const auto items_to_read = std::min(nmemb, remaining / size);
    const auto bytes_to_read = items_to_read * size;
    if (bytes_to_read == 0) {
        return 0;
    }
    std::memcpy(ptr, reader->bytes.data() + reader->offset, bytes_to_read);
    reader->offset += bytes_to_read;
    return items_to_read;
}

int VorbisSeekCallback(void* datasource, ogg_int64_t offset, int whence) {
    auto* reader = static_cast<VorbisMemoryReader*>(datasource);
    if (reader == nullptr) {
        return -1;
    }
    ogg_int64_t next = 0;
    if (whence == SEEK_SET) {
        next = offset;
    } else if (whence == SEEK_CUR) {
        next = static_cast<ogg_int64_t>(reader->offset) + offset;
    } else if (whence == SEEK_END) {
        next = static_cast<ogg_int64_t>(reader->bytes.size()) + offset;
    } else {
        return -1;
    }
    if (next < 0 || static_cast<std::size_t>(next) > reader->bytes.size()) {
        return -1;
    }
    reader->offset = static_cast<std::size_t>(next);
    return 0;
}

long VorbisTellCallback(void* datasource) {
    auto* reader = static_cast<VorbisMemoryReader*>(datasource);
    return reader == nullptr ? -1L : static_cast<long>(reader->offset);
}
#endif

#if defined(ASTRA_MEDIA_HAS_LIBJPEG)
struct JpegErrorManager {
    jpeg_error_mgr base;
    jmp_buf jump;
};

void JpegErrorExit(j_common_ptr info) {
    auto* manager = reinterpret_cast<JpegErrorManager*>(info->err);
    longjmp(manager->jump, 1);
}
#endif

class HeadlessRenderer2D final : public IRenderer2D {
public:
    void Submit(RenderGraph graph, Astra::Core::DiagnosticSink& diagnostics) override {
        std::ranges::sort(graph.draws, [](const RenderDraw& left, const RenderDraw& right) {
            if (left.layer == right.layer) {
                return left.order < right.order;
            }
            return left.layer < right.layer;
        });
        for (const auto& draw : graph.draws) {
            if (std::ranges::find(graph.layers, draw.layer) == graph.layers.end()) {
                diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_LAYER_UNKNOWN", Astra::Core::DiagnosticSeverity::Blocking, "Render draw references an unknown layer."));
            }
        }
        graph_ = std::move(graph);
    }

    FrameCapture Capture() const override {
        FrameCapture capture;
        capture.frame_index = graph_.frame_index;
        const auto graph_json = ToJson(graph_);
        capture.commands = graph_json;
        capture.render_hash = StableHash(graph_json.at("draws").dump());
        capture.text_hash = StableHash(graph_json.at("text_requests").dump());
        capture.audio_hash = StableHash(graph_json.at("audio_commands").dump());
        capture.filter_hash = StableHash(graph_json.at("filter_applications").dump());
        return capture;
    }

private:
    RenderGraph graph_;
};

} // namespace

std::unique_ptr<IRenderer2D> CreateHeadlessRenderer2D() {
    return std::make_unique<HeadlessRenderer2D>();
}

RenderGraph ExtractRenderGraph(const std::vector<PresentationCommand>& commands, const FilterProfile* filter_profile, Astra::Core::DiagnosticSink& diagnostics) {
    RenderGraph graph;
    for (const auto& command : commands) {
        graph.frame_index = std::max(graph.frame_index, command.frame_index);
        switch (command.kind) {
        case PresentationCommandKind::Sprite:
        case PresentationCommandKind::UiRect:
            graph.draws.push_back({command.command_id, command.asset, command.layer, command.order, command.transform, command.color});
            break;
        case PresentationCommandKind::Text:
            graph.text_requests.push_back({command.command_id, command.text, command.locale, command.layer, command.order, command.payload});
            break;
        case PresentationCommandKind::Audio:
            graph.audio_commands.push_back({command.command_id, command.payload.value("kind", "play"), command.asset, command.bus, command.payload.value("volume", 1.0), command.payload.value("loop", false)});
            break;
        case PresentationCommandKind::Filter:
            if (filter_profile == nullptr) {
                diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_FILTER_PROFILE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Filter presentation command requires a FilterProfile."));
            }
            break;
        case PresentationCommandKind::Timeline:
            break;
        }
    }
    if (filter_profile != nullptr) {
        auto validation = ValidateFilterProfile(*filter_profile, diagnostics);
        if (validation) {
            graph.filter_applications = ApplyFilterProfile(*filter_profile);
        }
    }
    return graph;
}

std::vector<MediaProviderDescriptor> FoundationMediaProviders() {
    auto capabilities = ProbeMediaBackendCapabilities();
    auto providers = std::vector<MediaProviderDescriptor>{
        {
            "astra.renderer2d.headless",
            Renderer2DSlotId,
            "Astra Headless Renderer2D",
            true,
            {"astra.foundation.sprite", "astra.foundation.ui_rect"},
            {"deterministic_hash", "frame_capture", "layer_sort", "filter_target_hash"},
            "ASTRA_MEDIA_RENDERER",
            "none",
        },
        {
            "astra.text_layout.foundation",
            TextLayoutSlotId,
            "Astra Foundation Text Layout",
            true,
            {"astra.foundation.text"},
            {"deterministic_hash", "logical_text_request"},
            "ASTRA_MEDIA_TEXT",
            "none",
        },
        {
            "astra.audio.foundation",
            AudioSlotId,
            "Astra Foundation Audio",
            true,
            {"astra.foundation.audio"},
            {"deterministic_hash", "logical_audio_command"},
            "ASTRA_MEDIA_AUDIO",
            "none",
        },
    };
    for (const auto& format : capabilities.image_formats) {
        AddUnique(providers[0].supported_formats, format);
    }
    for (const auto& feature : capabilities.font_features) {
        AddUnique(providers[1].features, feature);
    }
    for (const auto& feature : capabilities.audio_features) {
        AddUnique(providers[2].features, feature);
    }
    if (capabilities.image_decode_ready) {
        AddUnique(providers[0].features, "mature_image_decode_backend_available");
    }
    if (capabilities.text_layout_ready) {
        AddUnique(providers[1].features, "mature_font_shaping_backend_available");
    }
    if (capabilities.audio_mixer_ready) {
        AddUnique(providers[2].features, "mature_audio_mixer_backend_available");
    }
    return providers;
}

Astra::Core::Result<void> ValidateMediaProviderDescriptor(const MediaProviderDescriptor& descriptor, Astra::Core::DiagnosticSink& diagnostics) {
    bool valid = true;
    if (descriptor.provider_id.empty()) {
        EmitBlocking(diagnostics, "ASTRA_MEDIA_PROVIDER_ID_MISSING", "Media provider requires provider_id.");
        valid = false;
    }
    if (descriptor.slot_id.empty() || !IsRequiredMediaSlot(descriptor.slot_id)) {
        EmitBlocking(diagnostics, "ASTRA_MEDIA_PROVIDER_SLOT_INVALID", "Media provider slot_id must be astra.renderer2d, astra.text_layout, or astra.audio.", descriptor.provider_id);
        valid = false;
    }
    if (descriptor.display_name.empty()) {
        EmitBlocking(diagnostics, "ASTRA_MEDIA_PROVIDER_DISPLAY_MISSING", "Media provider requires display_name.", descriptor.provider_id);
        valid = false;
    }
    if (descriptor.supported_formats.empty()) {
        EmitBlocking(diagnostics, "ASTRA_MEDIA_PROVIDER_FORMATS_MISSING", "Media provider must declare at least one supported foundation format.", descriptor.provider_id);
        valid = false;
    }
    if (descriptor.features.empty()) {
        EmitBlocking(diagnostics, "ASTRA_MEDIA_PROVIDER_FEATURES_MISSING", "Media provider must declare backend features.", descriptor.provider_id);
        valid = false;
    }
    if (descriptor.diagnostics_code_prefix.empty()) {
        EmitBlocking(diagnostics, "ASTRA_MEDIA_PROVIDER_DIAGNOSTICS_MISSING", "Media provider must declare diagnostics_code_prefix.", descriptor.provider_id);
        valid = false;
    }
    if (descriptor.hot_reload_level.empty()) {
        EmitBlocking(diagnostics, "ASTRA_MEDIA_PROVIDER_HOT_RELOAD_MISSING", "Media provider must declare hot_reload_level.", descriptor.provider_id);
        valid = false;
    }
    return valid ? Astra::Core::Result<void>::Success() : Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "invalid media provider descriptor");
}

Astra::Core::Result<MediaReleaseGateReport> ValidateMediaReleaseGate(const MediaReleaseGateRequest& request, Astra::Core::DiagnosticSink& diagnostics) {
    bool valid = true;
    auto providers = request.providers.empty() ? FoundationMediaProviders() : request.providers;
    std::map<std::string, const MediaProviderDescriptor*> by_provider;
    for (const auto& provider : providers) {
        if (!ValidateMediaProviderDescriptor(provider, diagnostics)) {
            valid = false;
        }
        if (!provider.provider_id.empty()) {
            if (by_provider.contains(provider.provider_id)) {
                EmitBlocking(diagnostics, "ASTRA_MEDIA_PROVIDER_DUPLICATE", "Media provider_id must be unique.", provider.provider_id);
                valid = false;
            } else {
                by_provider.emplace(provider.provider_id, &provider);
            }
        }
    }

    std::map<std::string, std::string> selections = request.selected_providers;
    if (selections.empty()) {
        for (const auto& provider : providers) {
            if (IsRequiredMediaSlot(provider.slot_id) && !selections.contains(provider.slot_id)) {
                selections.emplace(provider.slot_id, provider.provider_id);
            }
        }
    }

    MediaReleaseGateReport report;
    for (const auto* slot : {Renderer2DSlotId, TextLayoutSlotId, AudioSlotId}) {
        const auto selected = selections.find(slot);
        if (selected == selections.end() || selected->second.empty()) {
            EmitBlocking(diagnostics, "ASTRA_MEDIA_RELEASE_SLOT_MISSING", std::string("Media release gate requires a selected provider for ") + slot + ".");
            valid = false;
            continue;
        }
        const auto provider = by_provider.find(selected->second);
        if (provider == by_provider.end()) {
            EmitBlocking(diagnostics, "ASTRA_MEDIA_RELEASE_PROVIDER_MISSING", "Selected media provider is not registered.", selected->second);
            valid = false;
            continue;
        }
        if (provider->second->slot_id != slot) {
            EmitBlocking(diagnostics, "ASTRA_MEDIA_RELEASE_SLOT_MISMATCH", "Selected media provider slot_id does not match the requested slot.", provider->second->provider_id);
            valid = false;
        }
        if (!provider->second->packaged_eligible) {
            EmitBlocking(diagnostics, "ASTRA_MEDIA_RELEASE_PROVIDER_NOT_PACKAGED", "Selected media provider is not packaged eligible.", provider->second->provider_id);
            valid = false;
        }
        report.selected_providers.push_back(*provider->second);
    }

    if (request.filter_profile.has_value()) {
        if (ValidateFilterProfile(*request.filter_profile, diagnostics)) {
            report.filter_applications = ApplyFilterProfile(*request.filter_profile);
        } else {
            valid = false;
        }
    }

    report.passed = valid;
    return valid ? Astra::Core::Result<MediaReleaseGateReport>::Success(std::move(report)) : Astra::Core::Result<MediaReleaseGateReport>::Failure(Astra::Core::ErrorCode::InvalidFormat, "media release gate failed");
}

MediaBackendCapabilityReport ProbeMediaBackendCapabilities() {
    MediaBackendCapabilityReport report;
    auto add_library = [&](MediaBackendLibrary library) {
        if (library.available) {
            for (const auto& format : library.formats) {
                if (format == "png" || format == "jpeg" || format == "webp") {
                    AddUnique(report.image_formats, format);
                }
            }
            for (const auto& feature : library.features) {
                if (feature.starts_with("font_") || feature == "text_shaping") {
                    AddUnique(report.font_features, feature);
                } else if (feature.starts_with("audio_")) {
                    AddUnique(report.audio_features, feature);
                }
            }
        }
        report.libraries.push_back(std::move(library));
    };

    add_library({
        "sdl3",
        "SDL3",
#if defined(ASTRA_ENABLE_SDL_BACKEND)
        true,
        std::to_string(SDL_MAJOR_VERSION) + "." + std::to_string(SDL_MINOR_VERSION) + "." + std::to_string(SDL_MICRO_VERSION),
        {"bmp"},
        {"window_surface", "texture_upload_path", "event_loop"},
#else
        false,
        "",
        {},
        {},
#endif
    });

    add_library({
        "libpng",
        "libpng",
#if defined(ASTRA_MEDIA_HAS_LIBPNG)
        true,
        PNG_LIBPNG_VER_STRING,
        {"png"},
        {"image_decode", "alpha"},
#else
        false,
        "",
        {},
        {},
#endif
    });

    add_library({
        "libjpeg-turbo",
        "libjpeg-turbo",
#if defined(ASTRA_MEDIA_HAS_LIBJPEG)
        true,
        std::to_string(JPEG_LIB_VERSION),
        {"jpeg"},
        {"image_decode", "ycbcr"},
#else
        false,
        "",
        {},
        {},
#endif
    });

    add_library({
        "libwebp",
        "libwebp",
#if defined(ASTRA_MEDIA_HAS_LIBWEBP)
        true,
        VersionFromPackedInteger(WebPGetDecoderVersion()),
        {"webp"},
        {"image_decode", "alpha"},
#else
        false,
        "",
        {},
        {},
#endif
    });

    add_library({
        "freetype",
        "FreeType",
#if defined(ASTRA_MEDIA_HAS_FREETYPE)
        true,
        std::to_string(FREETYPE_MAJOR) + "." + std::to_string(FREETYPE_MINOR) + "." + std::to_string(FREETYPE_PATCH),
        {"ttf", "otf"},
        {"font_rasterization", "font_metrics"},
#else
        false,
        "",
        {},
        {},
#endif
    });

    add_library({
        "harfbuzz",
        "HarfBuzz",
#if defined(ASTRA_MEDIA_HAS_HARFBUZZ)
        true,
        hb_version_string(),
        {},
        {"text_shaping", "font_fallback_ready"},
#else
        false,
        "",
        {},
        {},
#endif
    });

    add_library({
        "miniaudio",
        "miniaudio",
#if defined(ASTRA_MEDIA_HAS_MINIAUDIO)
        true,
        "available",
        {"wav", "flac", "mp3", "ogg", "vorbis"},
        {"audio_decode", "audio_mixer", "audio_streaming"},
#else
        false,
        "",
        {},
        {},
#endif
    });

    report.image_decode_ready = std::ranges::find(report.image_formats, "png") != report.image_formats.end()
                             && std::ranges::find(report.image_formats, "jpeg") != report.image_formats.end()
                             && std::ranges::find(report.image_formats, "webp") != report.image_formats.end();
    report.text_layout_ready = std::ranges::find(report.font_features, "font_rasterization") != report.font_features.end()
                            && std::ranges::find(report.font_features, "text_shaping") != report.font_features.end();
    report.audio_mixer_ready = std::ranges::find(report.audio_features, "audio_mixer") != report.audio_features.end();
    return report;
}

Astra::Core::Result<ImageDecodeReport> InspectImageBytes(std::span<const Astra::Core::u8> bytes, Astra::Core::DiagnosticSink& diagnostics) {
    if (bytes.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_IMAGE_EMPTY", Astra::Core::DiagnosticSeverity::Blocking, "Image payload is empty."));
        return Astra::Core::Result<ImageDecodeReport>::Failure(Astra::Core::ErrorCode::InvalidArgument, "image payload empty");
    }

    const auto starts_with = [&](std::initializer_list<Astra::Core::u8> magic) {
        if (bytes.size() < magic.size()) {
            return false;
        }
        return std::equal(magic.begin(), magic.end(), bytes.begin());
    };

    if (starts_with({0x89, 'P', 'N', 'G', 0x0d, 0x0a, 0x1a, 0x0a})) {
#if defined(ASTRA_MEDIA_HAS_LIBPNG)
        PngMemoryReader reader{bytes, 0};
        png_structp png = png_create_read_struct(PNG_LIBPNG_VER_STRING, nullptr, nullptr, nullptr);
        if (png == nullptr) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_IMAGE_BACKEND_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "libpng read struct could not be created."));
            return Astra::Core::Result<ImageDecodeReport>::Failure(Astra::Core::ErrorCode::InternalError, "libpng create failed");
        }
        png_infop info = png_create_info_struct(png);
        if (info == nullptr) {
            png_destroy_read_struct(&png, nullptr, nullptr);
            diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_IMAGE_BACKEND_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "libpng info struct could not be created."));
            return Astra::Core::Result<ImageDecodeReport>::Failure(Astra::Core::ErrorCode::InternalError, "libpng info failed");
        }
        if (setjmp(png_jmpbuf(png)) != 0) {
            png_destroy_read_struct(&png, &info, nullptr);
            diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_IMAGE_DECODE_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "libpng could not parse the image payload."));
            return Astra::Core::Result<ImageDecodeReport>::Failure(Astra::Core::ErrorCode::InvalidFormat, "png decode failed");
        }
        png_set_read_fn(png, &reader, PngReadCallback);
        png_read_info(png, info);
        png_uint_32 width = 0;
        png_uint_32 height = 0;
        int bit_depth = 0;
        int color_type = 0;
        int interlace = 0;
        int compression = 0;
        int filter = 0;
        png_get_IHDR(png, info, &width, &height, &bit_depth, &color_type, &interlace, &compression, &filter);
        ImageDecodeReport report;
        report.format = "png";
        report.width = static_cast<Astra::Core::u32>(width);
        report.height = static_cast<Astra::Core::u32>(height);
        report.channels = static_cast<Astra::Core::u32>(png_get_channels(png, info));
        report.has_alpha = (color_type & PNG_COLOR_MASK_ALPHA) != 0 || png_get_valid(png, info, PNG_INFO_tRNS) != 0;
        report.decoded_by = "libpng";
        png_destroy_read_struct(&png, &info, nullptr);
        return Astra::Core::Result<ImageDecodeReport>::Success(std::move(report));
#else
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_IMAGE_BACKEND_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "PNG image payload requires libpng support."));
        return Astra::Core::Result<ImageDecodeReport>::Failure(Astra::Core::ErrorCode::Unsupported, "libpng unavailable");
#endif
    }

    if (starts_with({0xff, 0xd8, 0xff})) {
#if defined(ASTRA_MEDIA_HAS_LIBJPEG)
        jpeg_decompress_struct jpeg{};
        JpegErrorManager error{};
        jpeg.err = jpeg_std_error(&error.base);
        error.base.error_exit = JpegErrorExit;
        if (setjmp(error.jump) != 0) {
            jpeg_destroy_decompress(&jpeg);
            diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_IMAGE_DECODE_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "libjpeg-turbo could not parse the image payload."));
            return Astra::Core::Result<ImageDecodeReport>::Failure(Astra::Core::ErrorCode::InvalidFormat, "jpeg decode failed");
        }
        jpeg_create_decompress(&jpeg);
        jpeg_mem_src(&jpeg, bytes.data(), static_cast<unsigned long>(bytes.size()));
        jpeg_read_header(&jpeg, TRUE);
        ImageDecodeReport report;
        report.format = "jpeg";
        report.width = static_cast<Astra::Core::u32>(jpeg.image_width);
        report.height = static_cast<Astra::Core::u32>(jpeg.image_height);
        report.channels = static_cast<Astra::Core::u32>(jpeg.num_components);
        report.has_alpha = false;
        report.decoded_by = "libjpeg-turbo";
        jpeg_destroy_decompress(&jpeg);
        return Astra::Core::Result<ImageDecodeReport>::Success(std::move(report));
#else
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_IMAGE_BACKEND_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "JPEG image payload requires libjpeg-turbo support."));
        return Astra::Core::Result<ImageDecodeReport>::Failure(Astra::Core::ErrorCode::Unsupported, "libjpeg unavailable");
#endif
    }

    if (bytes.size() >= 12 && std::memcmp(bytes.data(), "RIFF", 4) == 0 && std::memcmp(bytes.data() + 8, "WEBP", 4) == 0) {
#if defined(ASTRA_MEDIA_HAS_LIBWEBP)
        WebPBitstreamFeatures features{};
        const auto status = WebPGetFeatures(bytes.data(), bytes.size(), &features);
        if (status != VP8_STATUS_OK) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_IMAGE_DECODE_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "libwebp could not parse the image payload."));
            return Astra::Core::Result<ImageDecodeReport>::Failure(Astra::Core::ErrorCode::InvalidFormat, "webp decode failed");
        }
        ImageDecodeReport report;
        report.format = "webp";
        report.width = static_cast<Astra::Core::u32>(features.width);
        report.height = static_cast<Astra::Core::u32>(features.height);
        report.channels = features.has_alpha != 0 ? 4u : 3u;
        report.has_alpha = features.has_alpha != 0;
        report.decoded_by = "libwebp";
        return Astra::Core::Result<ImageDecodeReport>::Success(std::move(report));
#else
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_IMAGE_BACKEND_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "WebP image payload requires libwebp support."));
        return Astra::Core::Result<ImageDecodeReport>::Failure(Astra::Core::ErrorCode::Unsupported, "libwebp unavailable");
#endif
    }

    diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_IMAGE_FORMAT_UNSUPPORTED", Astra::Core::DiagnosticSeverity::Blocking, "Image payload is not PNG, JPEG, or WebP."));
    return Astra::Core::Result<ImageDecodeReport>::Failure(Astra::Core::ErrorCode::InvalidFormat, "unsupported image format");
}

Astra::Core::Result<DecodedImageRgba> DecodeImageRgbaBytes(std::span<const Astra::Core::u8> bytes, Astra::Core::DiagnosticSink& diagnostics) {
    if (bytes.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_IMAGE_EMPTY", Astra::Core::DiagnosticSeverity::Blocking, "Image payload is empty."));
        return Astra::Core::Result<DecodedImageRgba>::Failure(Astra::Core::ErrorCode::InvalidArgument, "image payload empty");
    }
    const auto is_png = bytes.size() >= 8
                     && bytes[0] == 0x89
                     && bytes[1] == 'P'
                     && bytes[2] == 'N'
                     && bytes[3] == 'G'
                     && bytes[4] == 0x0d
                     && bytes[5] == 0x0a
                     && bytes[6] == 0x1a
                     && bytes[7] == 0x0a;
    if (!is_png) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_IMAGE_FORMAT_UNSUPPORTED", Astra::Core::DiagnosticSeverity::Blocking, "RGBA decode currently supports PNG payloads."));
        return Astra::Core::Result<DecodedImageRgba>::Failure(Astra::Core::ErrorCode::InvalidFormat, "unsupported rgba image format");
    }

#if defined(ASTRA_MEDIA_HAS_LIBPNG)
    PngMemoryReader reader{bytes, 0};
    png_structp png = png_create_read_struct(PNG_LIBPNG_VER_STRING, nullptr, nullptr, nullptr);
    if (png == nullptr) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_IMAGE_BACKEND_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "libpng read struct could not be created."));
        return Astra::Core::Result<DecodedImageRgba>::Failure(Astra::Core::ErrorCode::InternalError, "libpng create failed");
    }
    png_infop info = png_create_info_struct(png);
    if (info == nullptr) {
        png_destroy_read_struct(&png, nullptr, nullptr);
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_IMAGE_BACKEND_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "libpng info struct could not be created."));
        return Astra::Core::Result<DecodedImageRgba>::Failure(Astra::Core::ErrorCode::InternalError, "libpng info failed");
    }
    if (setjmp(png_jmpbuf(png)) != 0) {
        png_destroy_read_struct(&png, &info, nullptr);
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_IMAGE_DECODE_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "libpng could not decode RGBA pixels."));
        return Astra::Core::Result<DecodedImageRgba>::Failure(Astra::Core::ErrorCode::InvalidFormat, "png rgba decode failed");
    }

    png_set_read_fn(png, &reader, PngReadCallback);
    png_read_info(png, info);

    png_uint_32 width = 0;
    png_uint_32 height = 0;
    int bit_depth = 0;
    int color_type = 0;
    int interlace = 0;
    int compression = 0;
    int filter = 0;
    png_get_IHDR(png, info, &width, &height, &bit_depth, &color_type, &interlace, &compression, &filter);

    if (bit_depth == 16) {
        png_set_strip_16(png);
    }
    if (color_type == PNG_COLOR_TYPE_PALETTE) {
        png_set_palette_to_rgb(png);
    }
    if (color_type == PNG_COLOR_TYPE_GRAY && bit_depth < 8) {
        png_set_expand_gray_1_2_4_to_8(png);
    }
    if (png_get_valid(png, info, PNG_INFO_tRNS) != 0) {
        png_set_tRNS_to_alpha(png);
    }
    if (color_type == PNG_COLOR_TYPE_GRAY || color_type == PNG_COLOR_TYPE_GRAY_ALPHA) {
        png_set_gray_to_rgb(png);
    }
    if ((color_type & PNG_COLOR_MASK_ALPHA) == 0 && png_get_valid(png, info, PNG_INFO_tRNS) == 0) {
        png_set_filler(png, 0xff, PNG_FILLER_AFTER);
    }

    png_read_update_info(png, info);
    const auto row_bytes = png_get_rowbytes(png, info);
    std::vector<Astra::Core::u8> pixels(static_cast<std::size_t>(row_bytes) * static_cast<std::size_t>(height));
    std::vector<png_bytep> rows(height);
    for (png_uint_32 row = 0; row < height; ++row) {
        rows[row] = pixels.data() + static_cast<std::size_t>(row) * row_bytes;
    }
    png_read_image(png, rows.data());
    png_read_end(png, nullptr);
    png_destroy_read_struct(&png, &info, nullptr);

    DecodedImageRgba decoded;
    decoded.format = "png";
    decoded.width = static_cast<Astra::Core::u32>(width);
    decoded.height = static_cast<Astra::Core::u32>(height);
    decoded.pixels = std::move(pixels);
    decoded.decoded_by = "libpng";
    return Astra::Core::Result<DecodedImageRgba>::Success(std::move(decoded));
#else
    diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_IMAGE_BACKEND_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "PNG RGBA decode requires libpng support."));
    return Astra::Core::Result<DecodedImageRgba>::Failure(Astra::Core::ErrorCode::Unsupported, "libpng unavailable");
#endif
}

Astra::Core::Result<RasterizedTextRgba> RasterizeTextRgbaBytes(std::span<const Astra::Core::u8> font_bytes, std::string_view text, Astra::Core::u32 pixel_height, Astra::Core::DiagnosticSink& diagnostics) {
    if (font_bytes.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_FONT_EMPTY", Astra::Core::DiagnosticSeverity::Blocking, "Font payload is empty."));
        return Astra::Core::Result<RasterizedTextRgba>::Failure(Astra::Core::ErrorCode::InvalidArgument, "font payload empty");
    }
    if (text.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_TEXT_EMPTY", Astra::Core::DiagnosticSeverity::Blocking, "Text payload is empty."));
        return Astra::Core::Result<RasterizedTextRgba>::Failure(Astra::Core::ErrorCode::InvalidArgument, "text empty");
    }
    if (pixel_height == 0) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_TEXT_SIZE_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Text pixel height must be greater than zero."));
        return Astra::Core::Result<RasterizedTextRgba>::Failure(Astra::Core::ErrorCode::InvalidArgument, "pixel height invalid");
    }

#if defined(ASTRA_MEDIA_HAS_FREETYPE)
    FT_Library library = nullptr;
    if (FT_Init_FreeType(&library) != 0) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_FREETYPE_INIT_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "FreeType could not be initialized."));
        return Astra::Core::Result<RasterizedTextRgba>::Failure(Astra::Core::ErrorCode::InternalError, "freetype init failed");
    }
    FT_Face face = nullptr;
    if (FT_New_Memory_Face(library, reinterpret_cast<const FT_Byte*>(font_bytes.data()), static_cast<FT_Long>(font_bytes.size()), 0, &face) != 0) {
        FT_Done_FreeType(library);
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_FONT_LOAD_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "FreeType could not load the font payload."));
        return Astra::Core::Result<RasterizedTextRgba>::Failure(Astra::Core::ErrorCode::InvalidFormat, "font load failed");
    }
    FT_Set_Pixel_Sizes(face, 0, pixel_height);

    struct ShapedGlyph {
        FT_UInt glyph_index = 0;
        int advance_x = 0;
        int advance_y = 0;
        int offset_x = 0;
        int offset_y = 0;
    };
    std::vector<ShapedGlyph> shaped;
    std::string shaped_by = "utf8_freetype";
#if defined(ASTRA_MEDIA_HAS_HARFBUZZ)
    hb_font_t* hb_font = hb_ft_font_create_referenced(face);
    hb_buffer_t* buffer = hb_buffer_create();
    hb_buffer_add_utf8(buffer, text.data(), static_cast<int>(text.size()), 0, static_cast<int>(text.size()));
    hb_buffer_guess_segment_properties(buffer);
    hb_shape(hb_font, buffer, nullptr, 0);
    unsigned int glyph_count = 0;
    const hb_glyph_info_t* infos = hb_buffer_get_glyph_infos(buffer, &glyph_count);
    const hb_glyph_position_t* positions = hb_buffer_get_glyph_positions(buffer, &glyph_count);
    shaped.reserve(glyph_count);
    for (unsigned int index = 0; index < glyph_count; ++index) {
        shaped.push_back({
            static_cast<FT_UInt>(infos[index].codepoint),
            static_cast<int>(positions[index].x_advance / 64),
            static_cast<int>(positions[index].y_advance / 64),
            static_cast<int>(positions[index].x_offset / 64),
            static_cast<int>(positions[index].y_offset / 64),
        });
    }
    hb_buffer_destroy(buffer);
    hb_font_destroy(hb_font);
    shaped_by = "harfbuzz";
#else
    for (const auto codepoint : DecodeUtf8Codepoints(text)) {
        const auto glyph_index = FT_Get_Char_Index(face, codepoint);
        if (FT_Load_Glyph(face, glyph_index, FT_LOAD_DEFAULT) == 0) {
            shaped.push_back({glyph_index, static_cast<int>(face->glyph->advance.x / 64), static_cast<int>(face->glyph->advance.y / 64), 0, 0});
        }
    }
#endif
    if (shaped.empty()) {
        FT_Done_Face(face);
        FT_Done_FreeType(library);
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_TEXT_SHAPE_EMPTY", Astra::Core::DiagnosticSeverity::Blocking, "Text shaping produced no glyphs."));
        return Astra::Core::Result<RasterizedTextRgba>::Failure(Astra::Core::ErrorCode::InvalidFormat, "text shape empty");
    }

    struct GlyphDraw {
        FT_UInt glyph_index = 0;
        int x = 0;
        int y = 0;
        int width = 0;
        int height = 0;
        std::vector<Astra::Core::u8> alpha;
    };
    std::vector<GlyphDraw> draws;
    int pen_x = 0;
    int pen_y = 0;
    int min_x = 0;
    int min_y = 0;
    int max_x = 0;
    int max_y = 0;
    bool has_bounds = false;
    for (const auto& glyph : shaped) {
        if (FT_Load_Glyph(face, glyph.glyph_index, FT_LOAD_DEFAULT) != 0 || FT_Render_Glyph(face->glyph, FT_RENDER_MODE_NORMAL) != 0) {
            pen_x += glyph.advance_x;
            pen_y += glyph.advance_y;
            continue;
        }
        const auto& bitmap = face->glyph->bitmap;
        const int x = pen_x + glyph.offset_x + face->glyph->bitmap_left;
        const int y = pen_y - glyph.offset_y - face->glyph->bitmap_top;
        GlyphDraw draw;
        draw.glyph_index = glyph.glyph_index;
        draw.x = x;
        draw.y = y;
        draw.width = static_cast<int>(bitmap.width);
        draw.height = static_cast<int>(bitmap.rows);
        draw.alpha.resize(static_cast<std::size_t>(draw.width) * static_cast<std::size_t>(draw.height));
        for (int row = 0; row < draw.height; ++row) {
            const auto* source = bitmap.buffer + row * bitmap.pitch;
            std::copy(source, source + draw.width, draw.alpha.begin() + static_cast<std::ptrdiff_t>(row * draw.width));
        }
        if (draw.width > 0 && draw.height > 0) {
            min_x = has_bounds ? std::min(min_x, x) : x;
            min_y = has_bounds ? std::min(min_y, y) : y;
            max_x = has_bounds ? std::max(max_x, x + draw.width) : x + draw.width;
            max_y = has_bounds ? std::max(max_y, y + draw.height) : y + draw.height;
            has_bounds = true;
        }
        draws.push_back(std::move(draw));
        pen_x += glyph.advance_x;
        pen_y += glyph.advance_y;
    }

    if (!has_bounds) {
        FT_Done_Face(face);
        FT_Done_FreeType(library);
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_TEXT_RASTER_EMPTY", Astra::Core::DiagnosticSeverity::Blocking, "Text rasterization produced no visible glyphs."));
        return Astra::Core::Result<RasterizedTextRgba>::Failure(Astra::Core::ErrorCode::InvalidFormat, "text raster empty");
    }

    RasterizedTextRgba raster;
    raster.width = static_cast<Astra::Core::u32>(std::max(1, max_x - min_x));
    raster.height = static_cast<Astra::Core::u32>(std::max(1, max_y - min_y));
    raster.glyph_count = static_cast<Astra::Core::u32>(draws.size());
    raster.pixel_height = pixel_height;
    raster.shaped_by = shaped_by;
    raster.rasterized_by = "freetype";
    raster.pixels.assign(static_cast<std::size_t>(raster.width) * raster.height * 4, 0);
    for (const auto& draw : draws) {
        for (int row = 0; row < draw.height; ++row) {
            for (int column = 0; column < draw.width; ++column) {
                const auto alpha = draw.alpha[static_cast<std::size_t>(row * draw.width + column)];
                if (alpha == 0) {
                    continue;
                }
                const auto target_x = static_cast<std::size_t>(draw.x - min_x + column);
                const auto target_y = static_cast<std::size_t>(draw.y - min_y + row);
                const auto offset = (target_y * raster.width + target_x) * 4;
                raster.pixels[offset + 0] = 245;
                raster.pixels[offset + 1] = 242;
                raster.pixels[offset + 2] = 230;
                raster.pixels[offset + 3] = std::max(raster.pixels[offset + 3], alpha);
            }
        }
    }
    FT_Done_Face(face);
    FT_Done_FreeType(library);
    return Astra::Core::Result<RasterizedTextRgba>::Success(std::move(raster));
#else
    diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_FREETYPE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Text rasterization requires FreeType support."));
    return Astra::Core::Result<RasterizedTextRgba>::Failure(Astra::Core::ErrorCode::Unsupported, "freetype unavailable");
#endif
}

Astra::Core::Result<AudioDecodeReport> DecodeAudioBytes(std::span<const Astra::Core::u8> bytes, Astra::Core::DiagnosticSink& diagnostics) {
    if (bytes.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_AUDIO_EMPTY", Astra::Core::DiagnosticSeverity::Blocking, "Audio payload is empty."));
        return Astra::Core::Result<AudioDecodeReport>::Failure(Astra::Core::ErrorCode::InvalidArgument, "audio payload empty");
    }
#if defined(ASTRA_MEDIA_HAS_FFMPEG)
    {
        Astra::Core::DiagnosticSink ffmpeg_diagnostics;
        auto decoded = DecodeAudioBytesWithFfmpeg(bytes, ffmpeg_diagnostics);
        if (decoded) {
            return decoded;
        }
    }
#endif
    const auto is_ogg = bytes.size() >= 4 && bytes[0] == 'O' && bytes[1] == 'g' && bytes[2] == 'g' && bytes[3] == 'S';
#if defined(ASTRA_MEDIA_HAS_VORBISFILE)
    if (is_ogg) {
        VorbisMemoryReader reader{bytes, 0};
        ov_callbacks callbacks{};
        callbacks.read_func = VorbisReadCallback;
        callbacks.seek_func = VorbisSeekCallback;
        callbacks.close_func = nullptr;
        callbacks.tell_func = VorbisTellCallback;
        OggVorbis_File file{};
        if (ov_open_callbacks(&reader, &file, nullptr, 0, callbacks) != 0) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_AUDIO_DECODE_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "libvorbisfile could not open the OGG/Vorbis payload."));
            return Astra::Core::Result<AudioDecodeReport>::Failure(Astra::Core::ErrorCode::InvalidFormat, "ogg decode failed");
        }
        const auto* info = ov_info(&file, -1);
        const auto length = ov_pcm_total(&file, -1);
        if (length <= 0) {
            ov_clear(&file);
            diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_AUDIO_DECODE_EMPTY", Astra::Core::DiagnosticSeverity::Blocking, "OGG decode produced no PCM frame length."));
            return Astra::Core::Result<AudioDecodeReport>::Failure(Astra::Core::ErrorCode::InvalidFormat, "ogg decode empty");
        }
        AudioDecodeReport report;
        report.format = "ogg";
        report.channels = info == nullptr ? 0 : static_cast<Astra::Core::u32>(info->channels);
        report.sample_rate = info == nullptr ? 0 : static_cast<Astra::Core::u32>(info->rate);
        report.pcm_frame_count = static_cast<Astra::Core::u64>(length);
        report.decoded_by = "libvorbisfile";
        ov_clear(&file);
        if (report.pcm_frame_count == 0 || report.channels == 0 || report.sample_rate == 0) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_AUDIO_DECODE_EMPTY", Astra::Core::DiagnosticSeverity::Blocking, "OGG decode produced no PCM frames."));
            return Astra::Core::Result<AudioDecodeReport>::Failure(Astra::Core::ErrorCode::InvalidFormat, "ogg decode empty");
        }
        return Astra::Core::Result<AudioDecodeReport>::Success(std::move(report));
    }
#endif

#if defined(ASTRA_MEDIA_HAS_MINIAUDIO)
    ma_decoder_config config = ma_decoder_config_init(ma_format_f32, 0, 0);
    ma_decoder decoder{};
    const auto init_result = ma_decoder_init_memory(bytes.data(), bytes.size(), &config, &decoder);
    if (init_result != MA_SUCCESS) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_AUDIO_DECODE_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "miniaudio could not initialize a decoder for the audio payload."));
        return Astra::Core::Result<AudioDecodeReport>::Failure(Astra::Core::ErrorCode::InvalidFormat, "audio decode failed");
    }
    ma_uint64 frame_count = 0;
    if (ma_decoder_get_length_in_pcm_frames(&decoder, &frame_count) != MA_SUCCESS) {
        std::vector<float> scratch(4096 * std::max<ma_uint32>(1, decoder.outputChannels));
        ma_uint64 total = 0;
        for (;;) {
            ma_uint64 frames_read = 0;
            const auto read = ma_decoder_read_pcm_frames(&decoder, scratch.data(), 4096, &frames_read);
            total += frames_read;
            if (read != MA_SUCCESS || frames_read == 0) {
                break;
            }
        }
        frame_count = total;
    }
    AudioDecodeReport report;
    report.format = "container";
    if (bytes.size() >= 4 && bytes[0] == 'O' && bytes[1] == 'g' && bytes[2] == 'g' && bytes[3] == 'S') {
        report.format = "ogg";
    }
    report.channels = decoder.outputChannels;
    report.sample_rate = decoder.outputSampleRate;
    report.pcm_frame_count = static_cast<Astra::Core::u64>(frame_count);
    report.decoded_by = "miniaudio";
    ma_decoder_uninit(&decoder);
    if (report.pcm_frame_count == 0 || report.channels == 0 || report.sample_rate == 0) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_AUDIO_DECODE_EMPTY", Astra::Core::DiagnosticSeverity::Blocking, "Audio decode produced no PCM frames."));
        return Astra::Core::Result<AudioDecodeReport>::Failure(Astra::Core::ErrorCode::InvalidFormat, "audio decode empty");
    }
    return Astra::Core::Result<AudioDecodeReport>::Success(std::move(report));
#else
    diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_MINIAUDIO_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Audio decode requires miniaudio support."));
    return Astra::Core::Result<AudioDecodeReport>::Failure(Astra::Core::ErrorCode::Unsupported, "miniaudio unavailable");
#endif
}

Astra::Core::Result<FilterProfile> FilterProfileFromJson(const nlohmann::json& json, Astra::Core::DiagnosticSink& diagnostics) {
    auto id = Astra::Asset::ParseAssetUri(json.value("id", ""));
    if (!id) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_FILTER_ID_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "FilterProfile id is invalid."));
        return Astra::Core::Result<FilterProfile>::Failure(id.Error(), id.Message());
    }
    FilterProfile profile;
    profile.schema = json.value("schema", FilterProfileSchema);
    profile.id = id.Value();
    for (const auto& pass_json : json.value("passes", nlohmann::json::array())) {
        auto target = FilterTargetFromString(pass_json.value("target", "final"));
        if (!target) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_FILTER_TARGET_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "FilterProfile pass has invalid target."));
            continue;
        }
        profile.passes.push_back({pass_json.value("id", ""), pass_json.value("filter", ""), target.Value(), pass_json.value("params", nlohmann::json::object())});
    }
    return Astra::Core::Result<FilterProfile>::Success(std::move(profile));
}

Astra::Core::Result<void> ValidateFilterProfile(const FilterProfile& profile, Astra::Core::DiagnosticSink& diagnostics) {
    bool valid = true;
    if (profile.id.path.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_FILTER_ID_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "FilterProfile id is required."));
        valid = false;
    }
    for (const auto& pass : profile.passes) {
        if (pass.id.empty() || pass.filter.empty()) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_MEDIA_FILTER_PASS_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "FilterProfile pass requires id and filter."));
            valid = false;
        }
    }
    return valid ? Astra::Core::Result<void>::Success() : Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "invalid filter profile");
}

std::vector<FilterApplication> ApplyFilterProfile(const FilterProfile& profile) {
    std::vector<FilterApplication> applications;
    for (const auto& pass : profile.passes) {
        applications.push_back({pass.id, pass.filter, pass.target, ToString(pass.target), StableHash(pass.params.dump())});
    }
    return applications;
}

std::string ToString(PresentationCommandKind kind) {
    switch (kind) {
    case PresentationCommandKind::Sprite:
        return "sprite";
    case PresentationCommandKind::Text:
        return "text";
    case PresentationCommandKind::UiRect:
        return "ui_rect";
    case PresentationCommandKind::Audio:
        return "audio";
    case PresentationCommandKind::Filter:
        return "filter";
    case PresentationCommandKind::Timeline:
        return "timeline";
    }
    return "sprite";
}

std::string ToString(FilterTarget target) {
    switch (target) {
    case FilterTarget::Background:
        return "background";
    case FilterTarget::Character:
        return "character";
    case FilterTarget::Ui:
        return "ui";
    case FilterTarget::Text:
        return "text";
    case FilterTarget::Final:
        return "final";
    }
    return "final";
}

Astra::Core::Result<FilterTarget> FilterTargetFromString(std::string_view value) {
    if (value == "background") {
        return Astra::Core::Result<FilterTarget>::Success(FilterTarget::Background);
    }
    if (value == "character") {
        return Astra::Core::Result<FilterTarget>::Success(FilterTarget::Character);
    }
    if (value == "ui") {
        return Astra::Core::Result<FilterTarget>::Success(FilterTarget::Ui);
    }
    if (value == "text") {
        return Astra::Core::Result<FilterTarget>::Success(FilterTarget::Text);
    }
    if (value == "final") {
        return Astra::Core::Result<FilterTarget>::Success(FilterTarget::Final);
    }
    return Astra::Core::Result<FilterTarget>::Failure(Astra::Core::ErrorCode::InvalidFormat, "unknown filter target");
}

nlohmann::json ToJson(const PresentationCommand& command) {
    return {
        {"kind", ToString(command.kind)},
        {"command_id", command.command_id},
        {"frame_index", command.frame_index},
        {"layer", command.layer},
        {"order", command.order},
        {"asset", command.asset.ToString()},
        {"transform", {{"x", command.transform.x}, {"y", command.transform.y}, {"rotation_degrees", command.transform.rotation_degrees}, {"scale_x", command.transform.scale_x}, {"scale_y", command.transform.scale_y}}},
        {"color", {command.color.r, command.color.g, command.color.b, command.color.a}},
        {"text", command.text},
        {"locale", command.locale},
        {"bus", command.bus},
        {"payload", command.payload},
    };
}

nlohmann::json ToJson(const RenderGraph& graph) {
    nlohmann::json draws = nlohmann::json::array();
    for (const auto& draw : graph.draws) {
        draws.push_back({
            {"draw_id", draw.draw_id},
            {"asset", draw.asset.ToString()},
            {"layer", draw.layer},
            {"order", draw.order},
            {"transform", {{"x", draw.transform.x}, {"y", draw.transform.y}, {"rotation_degrees", draw.transform.rotation_degrees}, {"scale_x", draw.transform.scale_x}, {"scale_y", draw.transform.scale_y}}},
            {"color", {draw.color.r, draw.color.g, draw.color.b, draw.color.a}},
        });
    }
    nlohmann::json texts = nlohmann::json::array();
    for (const auto& text : graph.text_requests) {
        texts.push_back({{"request_id", text.request_id}, {"text", text.text}, {"locale", text.locale}, {"layer", text.layer}, {"order", text.order}, {"style", text.style}});
    }
    nlohmann::json audio = nlohmann::json::array();
    for (const auto& command : graph.audio_commands) {
        audio.push_back({{"command_id", command.command_id}, {"kind", command.kind}, {"asset", command.asset.ToString()}, {"bus", command.bus}, {"volume", command.volume}, {"loop", command.loop}});
    }
    nlohmann::json filters = nlohmann::json::array();
    for (const auto& filter : graph.filter_applications) {
        filters.push_back({{"pass_id", filter.pass_id}, {"filter", filter.filter}, {"target", filter.target_name}, {"params_hash", filter.params_hash}});
    }
    return {{"frame_index", graph.frame_index}, {"layers", graph.layers}, {"draws", draws}, {"text_requests", texts}, {"audio_commands", audio}, {"filter_applications", filters}};
}

nlohmann::json ToJson(const FrameCapture& capture) {
    return {{"frame_index", capture.frame_index}, {"render_hash", capture.render_hash}, {"text_hash", capture.text_hash}, {"audio_hash", capture.audio_hash}, {"filter_hash", capture.filter_hash}, {"commands", capture.commands}};
}

nlohmann::json ToJson(const FilterProfile& profile) {
    nlohmann::json passes = nlohmann::json::array();
    for (const auto& pass : profile.passes) {
        passes.push_back({{"id", pass.id}, {"filter", pass.filter}, {"target", ToString(pass.target)}, {"params", pass.params}});
    }
    return {{"schema", profile.schema}, {"id", profile.id.ToString()}, {"passes", passes}};
}

nlohmann::json ToJson(const MediaProviderDescriptor& descriptor) {
    return {
        {"provider_id", descriptor.provider_id},
        {"slot_id", descriptor.slot_id},
        {"display_name", descriptor.display_name},
        {"packaged_eligible", descriptor.packaged_eligible},
        {"supported_formats", descriptor.supported_formats},
        {"features", descriptor.features},
        {"diagnostics_code_prefix", descriptor.diagnostics_code_prefix},
        {"hot_reload_level", descriptor.hot_reload_level},
    };
}

nlohmann::json ToJson(const MediaReleaseGateReport& report) {
    nlohmann::json providers = nlohmann::json::array();
    for (const auto& provider : report.selected_providers) {
        providers.push_back(ToJson(provider));
    }
    nlohmann::json filters = nlohmann::json::array();
    for (const auto& filter : report.filter_applications) {
        filters.push_back({{"pass_id", filter.pass_id}, {"filter", filter.filter}, {"target", filter.target_name}, {"params_hash", filter.params_hash}});
    }
    return {
        {"schema", report.schema},
        {"passed", report.passed},
        {"selected_providers", providers},
        {"filter_applications", filters},
    };
}

nlohmann::json ToJson(const MediaBackendLibrary& library) {
    return {
        {"id", library.id},
        {"display_name", library.display_name},
        {"available", library.available},
        {"version", library.version},
        {"formats", library.formats},
        {"features", library.features},
    };
}

nlohmann::json ToJson(const MediaBackendCapabilityReport& report) {
    nlohmann::json libraries = nlohmann::json::array();
    for (const auto& library : report.libraries) {
        libraries.push_back(ToJson(library));
    }
    return {
        {"schema", report.schema},
        {"libraries", libraries},
        {"image_formats", report.image_formats},
        {"font_features", report.font_features},
        {"audio_features", report.audio_features},
        {"image_decode_ready", report.image_decode_ready},
        {"text_layout_ready", report.text_layout_ready},
        {"audio_mixer_ready", report.audio_mixer_ready},
    };
}

nlohmann::json ToJson(const ImageDecodeReport& report) {
    return {
        {"schema", report.schema},
        {"format", report.format},
        {"width", report.width},
        {"height", report.height},
        {"channels", report.channels},
        {"has_alpha", report.has_alpha},
        {"decoded_by", report.decoded_by},
    };
}

} // namespace Astra::Media
