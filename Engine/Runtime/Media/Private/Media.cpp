#include <Astra/Media/Media.hpp>

#include <algorithm>
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
    return {
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

} // namespace Astra::Media
