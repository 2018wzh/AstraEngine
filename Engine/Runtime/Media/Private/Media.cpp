#include <Astra/Media/Media.hpp>

#include <Astra/Core/Logging.hpp>

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

std::string ToString(PresentationCommandKind kind);
std::string ToString(FilterTarget target);

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

bool IsKnownMediaSlot(std::string_view slot_id) {
    return IsRequiredMediaSlot(slot_id) || slot_id == ImageDecodeSlotId || slot_id == AudioDecodeSlotId
        || slot_id == VideoDecodeSlotId || slot_id == TimelineSlotId || slot_id == FilterGraphSlotId;
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
    Astra::Core::DefaultLogger().Log(
        "media.render",
        "render_graph",
        Astra::Core::LogLevel::Debug,
        "render graph extraction started",
        {{"commands", std::to_string(commands.size())},
         {"filter_profile", filter_profile == nullptr ? "" : filter_profile->id.ToString()}});
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
    Astra::Core::DefaultLogger().Log(
        "media.render",
        "render_graph",
        Astra::Core::LogLevel::Debug,
        "render graph extraction finished",
        {{"frame", std::to_string(graph.frame_index)},
         {"draws", std::to_string(graph.draws.size())},
         {"text", std::to_string(graph.text_requests.size())},
         {"audio", std::to_string(graph.audio_commands.size())},
         {"filters", std::to_string(graph.filter_applications.size())}});
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

std::vector<MediaProviderDescriptor> ProductionMediaProviders() {
    auto providers = FoundationMediaProviders();
    auto capabilities = ProbeMediaBackendCapabilities();
    providers[0].provider_id = "astra.renderer2d.bgfx";
    providers[0].display_name = "Astra bgfx Renderer2D";
    providers[0].features = {"texture_import", "sprite_batching", "render_target", "frame_capture", "clip_scissor", "device_recreate", "cross_api_backend"};
    if (!capabilities.renderer2d_ready) {
        AddUnique(providers[0].features, "bgfx_unavailable_at_build_time");
    }
    providers[0].hot_reload_level = "asset";
    providers[1].provider_id = "astra.text_layout.skia_ui";
    providers[1].display_name = "Astra Skia UI Text Raster";
    providers[1].features = {"ui_text_raster_to_texture", "text_shaping", "font_rasterization", "glyph_atlas", "fallback_font", "missing_glyph_diagnostics"};
    if (!capabilities.ui_text_raster_ready) {
        AddUnique(providers[1].features, "skia_unavailable_at_build_time");
    }
    providers[1].hot_reload_level = "asset";
    providers[2].provider_id = "astra.audio.miniaudio";
    providers[2].display_name = "Astra miniaudio Mixer";
    providers[2].features = {"audio_decode", "audio_mixer", "audio_streaming", "bus_routing", "pause_resume", "fade", "ducking", "silent_fallback"};
    providers[2].hot_reload_level = "asset";

    providers.push_back({"astra.decode.image.foundation", ImageDecodeSlotId, "Astra Image Decode Provider", true, capabilities.image_formats, {"cpu_decode", "package_payload"}, "ASTRA_DECODE_IMAGE", "asset"});
    providers.push_back({"astra.decode.audio.foundation", AudioDecodeSlotId, "Astra Audio Decode Provider", true, {"ogg", "wav", "flac", "mp3"}, {"metadata_decode", "package_payload"}, "ASTRA_DECODE_AUDIO", "asset"});
    providers.push_back({"astra.decode.video.ffmpeg", VideoDecodeSlotId, "Astra FFmpeg Video Decode Extension", true, {"mp4", "webm", "mkv", "ogv"}, {"extension_point", "no_fake_fallback"}, "ASTRA_DECODE_VIDEO", "asset"});
    providers.push_back({"astra.timeline.default", TimelineSlotId, "Astra Timeline Provider", true, {"astra.media.timeline.v1"}, {"camera_keys", "audio_events", "filter_events", "save_replay_state"}, "ASTRA_TIMELINE", "asset"});
    providers.push_back({"astra.filter_graph.bgfx", FilterGraphSlotId, "Astra FilterGraph Evidence", true, {"astra.media.filter_profile.v1"}, {"headless_hash_fallback", "gaussian_blur_descriptor", "line_enhance_descriptor", "color_grade_descriptor", "pass_through_descriptor"}, "ASTRA_FILTER_GRAPH", "asset"});
    return providers;
}

Astra::Core::Result<void> ValidateMediaProviderDescriptor(const MediaProviderDescriptor& descriptor, Astra::Core::DiagnosticSink& diagnostics) {
    bool valid = true;
    if (descriptor.provider_id.empty()) {
        EmitBlocking(diagnostics, "ASTRA_MEDIA_PROVIDER_ID_MISSING", "Media provider requires provider_id.");
        valid = false;
    }
    if (descriptor.slot_id.empty() || !IsKnownMediaSlot(descriptor.slot_id)) {
        EmitBlocking(diagnostics, "ASTRA_MEDIA_PROVIDER_SLOT_INVALID", "Media provider slot_id must be a known media or decode slot.", descriptor.provider_id);
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
    const auto capabilities = ProbeMediaBackendCapabilities();
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
    std::vector<const char*> required_slots = {Renderer2DSlotId, TextLayoutSlotId, AudioSlotId};
    for (const auto& provider : providers) {
        if (!IsRequiredMediaSlot(provider.slot_id)
            && std::ranges::find(required_slots, provider.slot_id) == required_slots.end()) {
            required_slots.push_back(provider.slot_id.c_str());
        }
    }

    for (const auto* slot : required_slots) {
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
        if (request.require_available_backends && provider->second->provider_id == "astra.renderer2d.bgfx" && !capabilities.renderer2d_ready) {
            EmitBlocking(diagnostics, "ASTRA_MEDIA_RELEASE_BGFX_UNAVAILABLE", "bgfx renderer provider was selected but bgfx is not available in this build.", provider->second->provider_id);
            valid = false;
        }
        if (request.require_available_backends && provider->second->provider_id == "astra.text_layout.skia_ui" && !capabilities.ui_text_raster_ready) {
            EmitBlocking(diagnostics, "ASTRA_MEDIA_RELEASE_SKIA_UNAVAILABLE", "Skia UI text provider was selected but Skia is not available in this build.", provider->second->provider_id);
            valid = false;
        }
        report.selected_providers.push_back(*provider->second);
        report.provider_hash_inputs.push_back(std::string(slot) + "=" + provider->second->provider_id);
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
    Astra::Core::DefaultLogger().Log(
        "media.backend",
        "capability_probe",
        Astra::Core::LogLevel::Debug,
        "media backend capability probe started");
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
        "bgfx",
        "bgfx",
#if defined(ASTRA_MEDIA_HAS_BGFX)
        true,
        "available",
        {"texture", "render_target"},
        {"renderer2d", "cross_api_backend", "sprite_batching"},
#else
        false,
        "",
        {},
        {},
#endif
    });

    add_library({
        "skia",
        "Skia",
#if defined(ASTRA_MEDIA_HAS_SKIA)
        true,
        "available",
        {"rgba8"},
        {"ui_text_raster", "canvas_raster"},
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
    report.renderer2d_ready =
        std::ranges::any_of(report.libraries, [](const MediaBackendLibrary& library) { return library.id == "bgfx" && library.available; });
    report.ui_text_raster_ready =
        std::ranges::any_of(report.libraries, [](const MediaBackendLibrary& library) { return library.id == "skia" && library.available; });
    Astra::Core::DefaultLogger().Log(
        "media.backend",
        "capability_probe",
        Astra::Core::LogLevel::Debug,
        "media backend capability probe finished",
        {{"libraries", std::to_string(report.libraries.size())},
         {"image_decode_ready", report.image_decode_ready ? "true" : "false"},
         {"text_layout_ready", report.text_layout_ready ? "true" : "false"},
         {"audio_mixer_ready", report.audio_mixer_ready ? "true" : "false"}});
    return report;
}
} // namespace Astra::Media
