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
#endif
#if defined(ASTRA_ENABLE_SDL_BACKEND)
#include <SDL3/SDL_version.h>
#endif

#include <algorithm>
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
        if (request.require_headless_fallback && !provider->second->headless_supported) {
            EmitBlocking(diagnostics, "ASTRA_MEDIA_RELEASE_HEADLESS_MISSING", "Selected media provider must support the Phase 3 headless fallback path.", provider->second->provider_id);
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
        {"wav", "flac", "mp3"},
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
        {"headless_supported", descriptor.headless_supported},
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
