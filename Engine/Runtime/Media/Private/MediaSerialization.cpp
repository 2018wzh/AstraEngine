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
        {"transform",
         {{"x", command.transform.x},
          {"y", command.transform.y},
          {"rotation_degrees", command.transform.rotation_degrees},
          {"scale_x", command.transform.scale_x},
          {"scale_y", command.transform.scale_y}}},
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
            {"transform",
             {{"x", draw.transform.x},
              {"y", draw.transform.y},
              {"rotation_degrees", draw.transform.rotation_degrees},
              {"scale_x", draw.transform.scale_x},
              {"scale_y", draw.transform.scale_y}}},
            {"color", {draw.color.r, draw.color.g, draw.color.b, draw.color.a}},
        });
    }
    nlohmann::json texts = nlohmann::json::array();
    for (const auto& text : graph.text_requests) {
        texts.push_back({{"request_id", text.request_id},
                         {"text", text.text},
                         {"locale", text.locale},
                         {"layer", text.layer},
                         {"order", text.order},
                         {"style", text.style}});
    }
    nlohmann::json audio = nlohmann::json::array();
    for (const auto& command : graph.audio_commands) {
        audio.push_back({{"command_id", command.command_id},
                         {"kind", command.kind},
                         {"asset", command.asset.ToString()},
                         {"bus", command.bus},
                         {"volume", command.volume},
                         {"loop", command.loop}});
    }
    nlohmann::json filters = nlohmann::json::array();
    for (const auto& filter : graph.filter_applications) {
        filters.push_back({{"pass_id", filter.pass_id},
                           {"filter", filter.filter},
                           {"target", filter.target_name},
                           {"params_hash", filter.params_hash}});
    }
    return {{"frame_index", graph.frame_index},
            {"layers", graph.layers},
            {"draws", draws},
            {"text_requests", texts},
            {"audio_commands", audio},
            {"filter_applications", filters}};
}

nlohmann::json ToJson(const FrameCapture& capture) {
    return {{"frame_index", capture.frame_index}, {"render_hash", capture.render_hash},
            {"text_hash", capture.text_hash},     {"audio_hash", capture.audio_hash},
            {"filter_hash", capture.filter_hash}, {"commands", capture.commands}};
}

nlohmann::json ToJson(const FilterProfile& profile) {
    nlohmann::json passes = nlohmann::json::array();
    for (const auto& pass : profile.passes) {
        passes.push_back({{"id", pass.id},
                          {"filter", pass.filter},
                          {"target", ToString(pass.target)},
                          {"params", pass.params}});
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
        filters.push_back({{"pass_id", filter.pass_id},
                           {"filter", filter.filter},
                           {"target", filter.target_name},
                           {"params_hash", filter.params_hash}});
    }
    return {
        {"schema", report.schema},
        {"passed", report.passed},
        {"selected_providers", providers},
        {"filter_applications", filters},
        {"provider_hash_inputs", report.provider_hash_inputs},
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
        {"renderer2d_ready", report.renderer2d_ready},
        {"ui_text_raster_ready", report.ui_text_raster_ready},
    };
}

nlohmann::json ToJson(const ImageDecodeReport& report) {
    return {
        {"schema", report.schema},         {"format", report.format},
        {"width", report.width},           {"height", report.height},
        {"channels", report.channels},     {"has_alpha", report.has_alpha},
        {"decoded_by", report.decoded_by},
    };
}

nlohmann::json ToJson(const DecodedCpuBuffer& buffer) {
    return {{"schema", buffer.schema},
            {"format", buffer.format},
            {"width", buffer.width},
            {"height", buffer.height},
            {"row_stride", buffer.row_stride},
            {"color_space", buffer.color_space},
            {"byte_count", buffer.pixels.size()}};
}

nlohmann::json ToJson(const GlyphRun& run) {
    return {{"schema", run.schema},
            {"request_id", run.request_id},
            {"locale", run.locale},
            {"glyph_count", run.glyph_count},
            {"missing_glyph_count", run.missing_glyph_count},
            {"run_hash", run.run_hash}};
}

nlohmann::json ToJson(const TextLayoutCapture& capture) {
    nlohmann::json runs = nlohmann::json::array();
    for (const auto& run : capture.glyph_runs) {
        runs.push_back(ToJson(run));
    }
    return {{"schema", capture.schema}, {"glyph_runs", runs}, {"glyph_hash", capture.glyph_hash}};
}

nlohmann::json ToJson(const AudioStateCapture& capture) {
    nlohmann::json commands = nlohmann::json::array();
    for (const auto& command : capture.active_commands) {
        commands.push_back({{"command_id", command.command_id},
                            {"kind", command.kind},
                            {"asset", command.asset.ToString()},
                            {"bus", command.bus},
                            {"volume", command.volume},
                            {"loop", command.loop}});
    }
    return {{"schema", capture.schema},
            {"bus_volumes", capture.bus_volumes},
            {"active_commands", commands},
            {"state_hash", capture.state_hash},
            {"silent_backend", capture.silent_backend}};
}

nlohmann::json ToJson(const RenderBackendSmoke& smoke) {
    return {
        {"schema", smoke.schema},
        {"renderer_provider", smoke.renderer_provider},
        {"text_provider", smoke.text_provider},
        {"renderer_available", smoke.renderer_available},
        {"text_available", smoke.text_available},
        {"presented", smoke.presented},
        {"imported_texture_count", smoke.imported_texture_count},
        {"draw_count", smoke.draw_count},
        {"text_texture_count", smoke.text_texture_count},
        {"frame_hash", smoke.frame_hash},
    };
}

nlohmann::json ToJson(const FrameCaptureRequest& request) {
    return {{"schema", request.schema},
            {"include_commands", request.include_commands},
            {"include_pixels", request.include_pixels},
            {"comparison_mode", request.comparison_mode}};
}

nlohmann::json ToJson(const AudioCaptureRequest& request) {
    return {{"schema", request.schema},
            {"include_active_commands", request.include_active_commands},
            {"include_pcm_checksum", request.include_pcm_checksum}};
}

nlohmann::json ToJson(const DriverDiffReport& report) {
    return {
        {"schema", report.schema},
        {"passed", report.passed},
        {"reference_provider", report.reference_provider},
        {"candidate_provider", report.candidate_provider},
        {"render_hash_reference", report.render_hash_reference},
        {"render_hash_candidate", report.render_hash_candidate},
        {"text_hash_reference", report.text_hash_reference},
        {"text_hash_candidate", report.text_hash_candidate},
        {"audio_hash_reference", report.audio_hash_reference},
        {"audio_hash_candidate", report.audio_hash_candidate},
        {"filter_hash_reference", report.filter_hash_reference},
        {"filter_hash_candidate", report.filter_hash_candidate},
        {"missing_required_capabilities", report.missing_required_capabilities},
    };
}

nlohmann::json ToJson(const VideoDecodeMetadata& metadata) {
    return {{"schema", metadata.schema},
            {"format", metadata.format},
            {"width", metadata.width},
            {"height", metadata.height},
            {"duration_seconds", metadata.duration_seconds},
            {"frame_rate", metadata.frame_rate},
            {"decoded_by", metadata.decoded_by}};
}

nlohmann::json ToJson(const DecodedVideoFrame& frame) {
    return {{"schema", frame.schema},
            {"frame_index", frame.frame_index},
            {"presentation_time_ns", frame.presentation_time_ns},
            {"output_kind", frame.output_kind},
            {"surface_token", frame.surface_token.id},
            {"cpu_buffer", ToJson(frame.cpu_buffer)},
            {"fallback_used", frame.fallback_used}};
}

nlohmann::json ToJson(const FilterGraphExecution& execution) {
    nlohmann::json applications = nlohmann::json::array();
    for (const auto& application : execution.applications) {
        applications.push_back({{"pass_id", application.pass_id},
                                {"filter", application.filter},
                                {"target", application.target_name},
                                {"params_hash", application.params_hash}});
    }
    return {{"schema", execution.schema},
            {"provider_id", execution.provider_id},
            {"execution_mode", execution.execution_mode},
            {"applications", applications},
            {"output_hash", execution.output_hash}};
}

nlohmann::json ToJson(const TimelineAsset& timeline) {
    nlohmann::json tracks = nlohmann::json::array();
    for (const auto& track : timeline.tracks) {
        nlohmann::json keys = nlohmann::json::array();
        for (const auto& key : track.keys) {
            keys.push_back({{"t", key.time_seconds}, {"value", key.value}, {"easing", key.easing}});
        }
        nlohmann::json audio = nlohmann::json::array();
        for (const auto& command : track.audio_events) {
            audio.push_back({{"command_id", command.command_id},
                             {"kind", command.kind},
                             {"asset", command.asset.ToString()},
                             {"bus", command.bus},
                             {"volume", command.volume},
                             {"loop", command.loop}});
        }
        nlohmann::json track_json = {
            {"id", track.id}, {"type", track.type}, {"keys", keys}, {"audio_events", audio}};
        if (track.filter_profile.has_value()) {
            track_json["filter_profile"] = ToJson(*track.filter_profile);
        }
        tracks.push_back(std::move(track_json));
    }
    return {{"schema", timeline.schema},
            {"id", timeline.id.ToString()},
            {"duration_seconds", timeline.duration_seconds},
            {"tracks", tracks}};
}

nlohmann::json ToJson(const TimelineState& state) {
    return {{"schema", state.schema},
            {"timeline_id", state.timeline_id.ToString()},
            {"cursor_time_ns", state.cursor_time_ns},
            {"active_tracks", state.active_tracks},
            {"pending_events", state.pending_events},
            {"camera", state.camera}};
}

} // namespace Astra::Media
