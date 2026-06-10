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

enum class PresentationCommandKind {
    Sprite,
    Text,
    UiRect,
    Audio,
    Filter,
    Timeline
};

enum class FilterTarget {
    Background,
    Character,
    Ui,
    Text,
    Final
};

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

struct MediaProviderDescriptor {
    std::string provider_id;
    std::string slot_id;
    std::string display_name;
    bool headless_supported = true;
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
    bool require_headless_fallback = true;
};

struct MediaReleaseGateReport {
    std::string schema = MediaReleaseGateSchema;
    bool passed = false;
    std::vector<MediaProviderDescriptor> selected_providers;
    std::vector<FilterApplication> filter_applications;
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

class ASTRA_MEDIA_API IRenderer2D {
public:
    virtual ~IRenderer2D() = default;
    virtual void Submit(RenderGraph graph, Astra::Core::DiagnosticSink& diagnostics) = 0;
    [[nodiscard]] virtual FrameCapture Capture() const = 0;
};

[[nodiscard]] ASTRA_MEDIA_API std::unique_ptr<IRenderer2D> CreateHeadlessRenderer2D();
[[nodiscard]] ASTRA_MEDIA_API std::unique_ptr<IRenderer2D> CreateSdlRenderer2DStub(Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API RenderGraph ExtractRenderGraph(const std::vector<PresentationCommand>& commands, const FilterProfile* filter_profile, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API std::vector<MediaProviderDescriptor> FoundationMediaProviders();
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<void> ValidateMediaProviderDescriptor(const MediaProviderDescriptor& descriptor, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<MediaReleaseGateReport> ValidateMediaReleaseGate(const MediaReleaseGateRequest& request, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API MediaBackendCapabilityReport ProbeMediaBackendCapabilities();
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<ImageDecodeReport> InspectImageBytes(std::span<const Astra::Core::u8> bytes, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<FilterProfile> FilterProfileFromJson(const nlohmann::json& json, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<void> ValidateFilterProfile(const FilterProfile& profile, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_MEDIA_API std::vector<FilterApplication> ApplyFilterProfile(const FilterProfile& profile);

[[nodiscard]] ASTRA_MEDIA_API std::string ToString(PresentationCommandKind kind);
[[nodiscard]] ASTRA_MEDIA_API std::string ToString(FilterTarget target);
[[nodiscard]] ASTRA_MEDIA_API Astra::Core::Result<FilterTarget> FilterTargetFromString(std::string_view value);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const PresentationCommand& command);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const RenderGraph& graph);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const FrameCapture& capture);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const FilterProfile& profile);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const MediaProviderDescriptor& descriptor);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const MediaReleaseGateReport& report);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const MediaBackendLibrary& library);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const MediaBackendCapabilityReport& report);
[[nodiscard]] ASTRA_MEDIA_API nlohmann::json ToJson(const ImageDecodeReport& report);

} // namespace Astra::Media
