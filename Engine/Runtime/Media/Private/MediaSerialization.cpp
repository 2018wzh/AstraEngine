#include <Astra/Media/Media.hpp>

namespace Astra::Media {

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
