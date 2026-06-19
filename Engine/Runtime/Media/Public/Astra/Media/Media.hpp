#pragma once

#include <Astra/Asset/Asset.hpp>
#include <Astra/Core/Diagnostics.hpp>
#include <Astra/Core/Types.hpp>
#include <Astra/Media/Export.hpp>
#include <nlohmann/json.hpp>

#include <map>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <vector>

namespace Astra::Media {

constexpr const char* FilterProfileSchema = "astra.media.filter_profile.v1";
constexpr const char* MediaReleaseGateSchema = "astra.media.release_gate.foundation.v1";
constexpr const char* Renderer2DSlotId = "astra.renderer2d";
constexpr const char* TextLayoutSlotId = "astra.text_layout";
constexpr const char* AudioSlotId = "astra.audio";
constexpr const char* ImageDecodeSlotId = "astra.image_decode";
constexpr const char* AudioDecodeSlotId = "astra.audio_decode";
constexpr const char* VideoDecodeSlotId = "astra.video_decode";
constexpr const char* TimelineSlotId = "astra.timeline";
constexpr const char* FilterGraphSlotId = "astra.filter_graph";

enum class PresentationCommandKind { Sprite, Text, UiRect, Audio, Filter, Timeline };

enum class FilterTarget { Background, Character, Ui, Text, Final };

struct Transform2D {
    double x = 0.0;
    double y = 0.0;
    double rotation_degrees = 0.0;
    double scale_x = 1.0;
    double scale_y = 1.0;
};

struct ColorRgba {
    double r = 1.0;
    double g = 1.0;
    double b = 1.0;
    double a = 1.0;
};

struct PresentationCommand {
    PresentationCommandKind kind = PresentationCommandKind::Sprite;
    std::string command_id;
    Astra::Core::u64 frame_index = 0;
    std::string layer = "background";
    Astra::Core::i32 order = 0;
    Astra::Asset::AssetUri asset;
    Transform2D transform;
    ColorRgba color;
    std::string text;
    std::string locale;
    std::string bus;
    nlohmann::json payload = nlohmann::json::object();
};

struct RenderDraw {
    std::string draw_id;
    Astra::Asset::AssetUri asset;
    std::string layer = "background";
    Astra::Core::i32 order = 0;
    Transform2D transform;
    ColorRgba color;
};

struct TextLayoutRequest {
    std::string request_id;
    std::string text;
    std::string locale;
    std::string layer = "text";
    Astra::Core::i32 order = 0;
    nlohmann::json style = nlohmann::json::object();
};

struct AudioCommand {
    std::string command_id;
    std::string kind = "play";
    Astra::Asset::AssetUri asset;
    std::string bus = "sfx";
    double volume = 1.0;
    bool loop = false;
};

struct RenderFrameDesc {
    Astra::Core::u64 frame_index = 0;
    Astra::Core::u32 width = 1280;
    Astra::Core::u32 height = 720;
    std::string color_space = "srgb";
};

struct PresentRequest {
    std::string target_id = "target:/main";
    bool allow_headless = true;
};

struct RenderTargetBinding {
    Astra::Core::u64 id = 0;
    std::string backend = "headless";
    Astra::Core::u32 width = 0;
    Astra::Core::u32 height = 0;

    [[nodiscard]] bool Empty() const {
        return id == 0;
    }
};

struct TextureToken {
    Astra::Core::u64 id = 0;
    [[nodiscard]] bool Empty() const {
        return id == 0;
    }
};

struct GlyphAtlasToken {
    Astra::Core::u64 id = 0;
    [[nodiscard]] bool Empty() const {
        return id == 0;
    }
};

struct AudioStreamToken {
    Astra::Core::u64 id = 0;
    [[nodiscard]] bool Empty() const {
        return id == 0;
    }
};

struct DecodeStreamToken {
    Astra::Core::u64 id = 0;
    [[nodiscard]] bool Empty() const {
        return id == 0;
    }
};

struct MediaSurfaceToken {
    Astra::Core::u64 id = 0;
    [[nodiscard]] bool Empty() const {
        return id == 0;
    }
};

struct DecodedCpuBuffer {
    std::string schema = "astra.media.decoded_cpu_buffer.v1";
    std::string format = "rgba8";
    Astra::Core::u32 width = 0;
    Astra::Core::u32 height = 0;
    Astra::Core::u32 row_stride = 0;
    std::string color_space = "srgb";
    std::vector<Astra::Core::u8> pixels;
};

struct VideoDecodeMetadata {
    std::string schema = "astra.media.video_decode.v1";
    std::string format;
    Astra::Core::u32 width = 0;
    Astra::Core::u32 height = 0;
    double duration_seconds = 0.0;
    double frame_rate = 0.0;
    std::string decoded_by;
};

struct DecodedVideoFrame {
    std::string schema = "astra.media.decoded_video_frame.v1";
    Astra::Core::u64 frame_index = 0;
    Astra::Core::u64 presentation_time_ns = 0;
    std::string output_kind = "cpu_rgba";
    MediaSurfaceToken surface_token;
    DecodedCpuBuffer cpu_buffer;
    bool fallback_used = false;
};

struct FilterPass {
    std::string id;
    std::string filter;
    FilterTarget target = FilterTarget::Final;
    nlohmann::json params = nlohmann::json::object();
};

struct FilterProfile {
    std::string schema = FilterProfileSchema;
    Astra::Asset::AssetUri id;
    std::vector<FilterPass> passes;
};

struct FilterApplication {
    std::string pass_id;
    std::string filter;
    FilterTarget target = FilterTarget::Final;
    std::string target_name;
    std::string params_hash;
};

struct FilterGraphExecution {
    std::string schema = "astra.media.filter_graph_execution.v1";
    std::vector<FilterApplication> applications;
    std::string provider_id;
    std::string execution_mode = "headless_hash";
    std::string output_hash;
};

struct GlyphRun {
    std::string schema = "astra.media.glyph_run.v1";
    std::string request_id;
    std::string locale;
    Astra::Core::u32 glyph_count = 0;
    Astra::Core::u32 missing_glyph_count = 0;
    std::string run_hash;
};

struct TextLayoutCapture {
    std::string schema = "astra.media.text_layout_capture.v1";
    std::vector<GlyphRun> glyph_runs;
    std::string glyph_hash;
};

struct AudioStateCapture {
    std::string schema = "astra.media.audio_state_capture.v1";
    std::map<std::string, double> bus_volumes;
    std::vector<AudioCommand> active_commands;
    std::string state_hash;
    bool silent_backend = false;
};

struct TimelineKeyframe {
    double time_seconds = 0.0;
    nlohmann::json value = nlohmann::json::object();
    std::string easing = "linear";
};

struct TimelineTrack {
    std::string id;
    std::string type;
    std::vector<TimelineKeyframe> keys;
    std::vector<AudioCommand> audio_events;
    std::optional<FilterProfile> filter_profile;
};

struct TimelineAsset {
    std::string schema = "astra.media.timeline.v1";
    Astra::Asset::AssetUri id;
    double duration_seconds = 0.0;
    std::vector<TimelineTrack> tracks;
};

struct TimelineState {
    std::string schema = "astra.media.timeline_state.v1";
    Astra::Asset::AssetUri timeline_id;
    Astra::Core::u64 cursor_time_ns = 0;
    std::vector<std::string> active_tracks;
    std::vector<std::string> pending_events;
    nlohmann::json camera = nlohmann::json::object();
};

struct RenderGraph {
    Astra::Core::u64 frame_index = 0;
    std::vector<std::string> layers = {"background", "character", "ui", "text", "final"};
    std::vector<RenderDraw> draws;
    std::vector<TextLayoutRequest> text_requests;
    std::vector<AudioCommand> audio_commands;
    std::vector<FilterApplication> filter_applications;
};

struct FrameCapture {
    Astra::Core::u64 frame_index = 0;
    std::string render_hash;
    std::string text_hash;
    std::string audio_hash;
    std::string filter_hash;
    nlohmann::json commands = nlohmann::json::array();
};

struct FrameCaptureRequest {
    std::string schema = "astra.media.frame_capture_request.v1";
    bool include_commands = true;
    bool include_pixels = false;
    std::string comparison_mode = "logical_hash";
};

struct AudioCaptureRequest {
    std::string schema = "astra.media.audio_capture_request.v1";
    bool include_active_commands = true;
    bool include_pcm_checksum = true;
};

struct DriverDiffReport {
    std::string schema = "astra.media.driver_diff.v1";
    bool passed = false;
    std::string reference_provider;
    std::string candidate_provider;
    std::string render_hash_reference;
    std::string render_hash_candidate;
    std::string text_hash_reference;
    std::string text_hash_candidate;
    std::string audio_hash_reference;
    std::string audio_hash_candidate;
    std::string filter_hash_reference;
    std::string filter_hash_candidate;
    std::vector<std::string> missing_required_capabilities;
};

struct MediaProviderDescriptor {
    std::string provider_id;
    std::string slot_id;
    std::string display_name;
    bool packaged_eligible = true;
    std::vector<std::string> supported_formats;
    std::vector<std::string> features;
    std::string diagnostics_code_prefix;
    std::string hot_reload_level = "none";
};

struct MediaReleaseGateRequest {
    std::map<std::string, std::string> selected_providers;
    std::vector<MediaProviderDescriptor> providers;
    std::optional<FilterProfile> filter_profile;
    bool require_available_backends = false;
};

struct MediaReleaseGateReport {
    std::string schema = MediaReleaseGateSchema;
    bool passed = false;
    std::vector<MediaProviderDescriptor> selected_providers;
    std::vector<FilterApplication> filter_applications;
    std::vector<std::string> provider_hash_inputs;
};

struct MediaBackendLibrary {
    std::string id;
    std::string display_name;
    bool available = false;
    std::string version;
    std::vector<std::string> formats;
    std::vector<std::string> features;
};

struct MediaBackendCapabilityReport {
    std::string schema = "astra.media.backend_capabilities.v1";
    std::vector<MediaBackendLibrary> libraries;
    std::vector<std::string> image_formats;
    std::vector<std::string> font_features;
    std::vector<std::string> audio_features;
    bool image_decode_ready = false;
    bool text_layout_ready = false;
    bool audio_mixer_ready = false;
    bool renderer2d_ready = false;
    bool ui_text_raster_ready = false;
};

struct ImageDecodeReport {
    std::string schema = "astra.media.image_decode.v1";
    std::string format;
    Astra::Core::u32 width = 0;
    Astra::Core::u32 height = 0;
    Astra::Core::u32 channels = 0;
    bool has_alpha = false;
    std::string decoded_by;
};

struct DecodedImageRgba {
    std::string schema = "astra.media.decoded_image_rgba.v1";
    std::string format;
    Astra::Core::u32 width = 0;
    Astra::Core::u32 height = 0;
    std::vector<Astra::Core::u8> pixels;
    std::string decoded_by;
};

struct RasterizedTextRgba {
    std::string schema = "astra.media.rasterized_text_rgba.v1";
    Astra::Core::u32 width = 0;
    Astra::Core::u32 height = 0;
    Astra::Core::u32 glyph_count = 0;
    Astra::Core::u32 pixel_height = 0;
    std::vector<Astra::Core::u8> pixels;
    std::string shaped_by;
    std::string rasterized_by;
};

struct RenderBackendValidation {
    std::string schema = "astra.media.render_backend_validation.v1";
    std::string renderer_provider;
    std::string text_provider;
    bool renderer_available = false;
    bool text_available = false;
    bool presented = false;
    Astra::Core::u32 imported_texture_count = 0;
    Astra::Core::u32 draw_count = 0;
    Astra::Core::u32 text_texture_count = 0;
    std::string frame_hash;
};

struct AudioDecodeReport {
    std::string schema = "astra.media.audio_decode.v1";
    std::string format;
    Astra::Core::u32 channels = 0;
    Astra::Core::u32 sample_rate = 0;
    Astra::Core::u64 pcm_frame_count = 0;
    std::string decoded_by;
};

class ASTRA_MEDIA_API IRenderer2D {
  public:
    virtual ~IRenderer2D() = default;
    virtual void Submit(RenderGraph graph, Astra::Core::DiagnosticSink& diagnostics) = 0;
    [[nodiscard]] virtual FrameCapture Capture() const = 0;
};

class ASTRA_MEDIA_API IRenderer2DProvider {
  public:
    virtual ~IRenderer2DProvider() = default;
    [[nodiscard]] virtual MediaProviderDescriptor Describe() const = 0;
    [[nodiscard]] virtual Astra::Core::Result<void>
    BeginFrame(const RenderFrameDesc& desc, Astra::Core::DiagnosticSink& diagnostics) = 0;
    [[nodiscard]] virtual Astra::Core::Result<TextureToken>
    ImportTexture(const DecodedCpuBuffer& buffer, Astra::Core::DiagnosticSink& diagnostics) = 0;
    [[nodiscard]] virtual Astra::Core::Result<TextureToken>
    ImportSurface(MediaSurfaceToken token, Astra::Core::DiagnosticSink& diagnostics) = 0;
    [[nodiscard]] virtual Astra::Core::Result<void>
    Execute(const RenderGraph& graph, Astra::Core::DiagnosticSink& diagnostics) = 0;
    [[nodiscard]] virtual Astra::Core::Result<FrameCapture>
    Capture(Astra::Core::DiagnosticSink& diagnostics) = 0;
    [[nodiscard]] virtual Astra::Core::Result<void>
    Present(PresentRequest request, Astra::Core::DiagnosticSink& diagnostics) = 0;
};

class ASTRA_MEDIA_API ITextLayoutProvider {
  public:
    virtual ~ITextLayoutProvider() = default;
    [[nodiscard]] virtual MediaProviderDescriptor Describe() const = 0;
    [[nodiscard]] virtual Astra::Core::Result<GlyphRun>
    Shape(TextLayoutRequest request, Astra::Core::DiagnosticSink& diagnostics) = 0;
    [[nodiscard]] virtual Astra::Core::Result<GlyphAtlasToken>
    PrepareAtlas(const GlyphRun& run, Astra::Core::DiagnosticSink& diagnostics) = 0;
    [[nodiscard]] virtual Astra::Core::Result<TextLayoutCapture>
    Capture(Astra::Core::DiagnosticSink& diagnostics) = 0;
};

class ASTRA_MEDIA_API IAudioProvider {
  public:
    virtual ~IAudioProvider() = default;
    [[nodiscard]] virtual MediaProviderDescriptor Describe() const = 0;
    [[nodiscard]] virtual Astra::Core::Result<AudioStreamToken>
    CreateStream(const AudioCommand& command, Astra::Core::DiagnosticSink& diagnostics) = 0;
    [[nodiscard]] virtual Astra::Core::Result<void>
    Submit(const std::vector<AudioCommand>& commands, Astra::Core::DiagnosticSink& diagnostics) = 0;
    [[nodiscard]] virtual Astra::Core::Result<AudioStateCapture>
    Capture(Astra::Core::DiagnosticSink& diagnostics) = 0;
};

[[nodiscard]] ASTRA_MEDIA_API std::unique_ptr<IRenderer2D> CreateHeadlessRenderer2D();
[[nodiscard]] ASTRA_MEDIA_API std::unique_ptr<IRenderer2DProvider>
CreateHeadlessRenderer2DProvider();
[[nodiscard]] ASTRA_MEDIA_API std::unique_ptr<IRenderer2DProvider>
CreateProductionRenderer2DProvider(RenderTargetBinding binding = {});
[[nodiscard]] ASTRA_MEDIA_API std::unique_ptr<ITextLayoutProvider>
CreateFoundationTextLayoutProvider();
[[nodiscard]] ASTRA_MEDIA_API std::unique_ptr<ITextLayoutProvider>
CreateProductionTextLayoutProvider();
[[nodiscard]] ASTRA_MEDIA_API std::unique_ptr<IAudioProvider>
CreateFoundationAudioProvider(bool silent_backend = true);
[[nodiscard]] ASTRA_MEDIA_API RenderGraph
ExtractRenderGraph(const std::vector<PresentationCommand>& commands,
                   const FilterProfile* filter_profile, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API std::vector<MediaProviderDescriptor> FoundationMediaProviders();
[[nodiscard]] ASTRA_MEDIA_API std::vector<MediaProviderDescriptor> ProductionMediaProviders();
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<void>
ValidateMediaProviderDescriptor(const MediaProviderDescriptor& descriptor,
                                Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<MediaReleaseGateReport>
ValidateMediaReleaseGate(const MediaReleaseGateRequest& request,
                         Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API MediaBackendCapabilityReport ProbeMediaBackendCapabilities();
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<ImageDecodeReport>
InspectImageBytes(std::span<const Astra::Core::u8> bytes, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<DecodedImageRgba>
DecodeImageRgbaBytes(std::span<const Astra::Core::u8> bytes,
                     Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<DecodedCpuBuffer>
DecodeImageCpuBufferBytes(std::span<const Astra::Core::u8> bytes,
                          Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<RasterizedTextRgba>
RasterizeTextRgbaBytes(std::span<const Astra::Core::u8> font_bytes, std::string_view text,
                       Astra::Core::u32 pixel_height, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<DecodedCpuBuffer>
RasterizeUiTextCpuBufferBytes(std::span<const Astra::Core::u8> font_bytes, std::string_view text,
                              Astra::Core::u32 pixel_height,
                              Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<AudioDecodeReport>
DecodeAudioBytes(std::span<const Astra::Core::u8> bytes, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<VideoDecodeMetadata>
InspectVideoBytes(std::span<const Astra::Core::u8> bytes, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<DecodedVideoFrame>
DecodeVideoFrameRgbaBytes(std::span<const Astra::Core::u8> bytes, Astra::Core::u64 frame_index,
                          Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<FilterProfile>
FilterProfileFromJson(const nlohmann::json& json, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<void>
ValidateFilterProfile(const FilterProfile& profile, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API std::vector<FilterApplication>
ApplyFilterProfile(const FilterProfile& profile);
[[nodiscard]] ASTRA_MEDIA_API FilterGraphExecution
ExecuteFilterGraphHeadless(const FilterProfile& profile, const RenderGraph& graph);
[[nodiscard]] ASTRA_MEDIA_API FilterGraphExecution
ExecuteFilterGraphCpu(const FilterProfile& profile, const RenderGraph& graph,
                      DecodedCpuBuffer* buffer, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API DriverDiffReport CompareDriverCaptures(
    const FrameCapture& reference, const FrameCapture& candidate,
    const MediaBackendCapabilityReport& capabilities, bool require_driver_hardening);
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<TimelineAsset>
TimelineFromJson(const nlohmann::json& json, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<void>
ValidateTimeline(const TimelineAsset& timeline, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API TimelineState EvaluateTimeline(const TimelineAsset& timeline,
                                                             Astra::Core::u64 cursor_time_ns);

[[nodiscard]] ASTRA_MEDIA_API std::string ToString(PresentationCommandKind kind);
[[nodiscard]] ASTRA_MEDIA_API std::string ToString(FilterTarget target);
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<FilterTarget>
FilterTargetFromString(std::string_view value);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const PresentationCommand& command);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const RenderGraph& graph);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const FrameCapture& capture);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const FilterProfile& profile);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const MediaProviderDescriptor& descriptor);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const MediaReleaseGateReport& report);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const MediaBackendLibrary& library);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const MediaBackendCapabilityReport& report);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const ImageDecodeReport& report);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const DecodedCpuBuffer& buffer);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const GlyphRun& run);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const TextLayoutCapture& capture);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const AudioStateCapture& capture);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const RenderBackendValidation& validation);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const FrameCaptureRequest& request);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const AudioCaptureRequest& request);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const DriverDiffReport& report);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const VideoDecodeMetadata& metadata);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const DecodedVideoFrame& frame);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const FilterGraphExecution& execution);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const TimelineAsset& timeline);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const TimelineState& state);

} // namespace Astra::Media


