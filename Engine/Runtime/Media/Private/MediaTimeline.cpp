#include <Astra/Media/Media.hpp>

#include "MediaInternal.hpp"

#include <algorithm>
#include <ranges>

namespace Astra::Media {

namespace {

Astra::Core::Diagnostic MakeDiagnostic(std::string code, Astra::Core::DiagnosticSeverity severity, std::string message) {
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = std::move(code);
    diagnostic.category = "media.timeline";
    diagnostic.severity = severity;
    diagnostic.message = std::move(message);
    return diagnostic;
}

double CursorSeconds(Astra::Core::u64 cursor_time_ns) {
    return static_cast<double>(cursor_time_ns) / 1000000000.0;
}

} // namespace

FilterGraphExecution ExecuteFilterGraphHeadless(const FilterProfile& profile, const RenderGraph& graph) {
    FilterGraphExecution execution;
    execution.provider_id = "astra.filter_graph.sdl_gpu";
    execution.execution_mode = "headless_hash_fallback";
    execution.applications = ApplyFilterProfile(profile);
    nlohmann::json json = nlohmann::json::array();
    for (const auto& application : execution.applications) {
        json.push_back({{"pass_id", application.pass_id}, {"filter", application.filter}, {"target", application.target_name}, {"params_hash", application.params_hash}});
    }
    execution.output_hash = Private::StableHash(json.dump() + ToJson(graph).dump());
    return execution;
}

Astra::Core::Result<TimelineAsset> TimelineFromJson(const nlohmann::json& json, Astra::Core::DiagnosticSink& diagnostics) {
    auto id = Astra::Asset::ParseAssetUri(json.value("id", ""));
    if (!id) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_TIMELINE_ID_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Timeline id is invalid."));
        return Astra::Core::Result<TimelineAsset>::Failure(id.Error(), id.Message());
    }
    TimelineAsset timeline;
    timeline.schema = json.value("schema", timeline.schema);
    timeline.id = id.Value();
    timeline.duration_seconds = json.value("duration_seconds", 0.0);
    for (const auto& track_json : json.value("tracks", nlohmann::json::array())) {
        TimelineTrack track;
        track.id = track_json.value("id", "");
        track.type = track_json.value("type", "");
        for (const auto& key_json : track_json.value("keys", nlohmann::json::array())) {
            track.keys.push_back({key_json.value("t", 0.0), key_json.value("value", nlohmann::json::object()), key_json.value("easing", "linear")});
        }
        for (const auto& audio_json : track_json.value("audio_events", nlohmann::json::array())) {
            auto asset = Astra::Asset::ParseAssetUri(audio_json.value("asset", ""));
            if (!asset) {
                diagnostics.Emit(MakeDiagnostic("ASTRA_TIMELINE_AUDIO_ASSET_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Timeline audio event has invalid asset."));
                continue;
            }
            track.audio_events.push_back({audio_json.value("command_id", track.id), audio_json.value("kind", "play"), asset.Value(), audio_json.value("bus", "music"), audio_json.value("volume", 1.0), audio_json.value("loop", false)});
        }
        if (track_json.contains("filter_profile")) {
            auto profile = FilterProfileFromJson(track_json["filter_profile"], diagnostics);
            if (profile) {
                track.filter_profile = profile.Value();
            }
        }
        timeline.tracks.push_back(std::move(track));
    }
    auto valid = ValidateTimeline(timeline, diagnostics);
    if (!valid) {
        return Astra::Core::Result<TimelineAsset>::Failure(valid.Error(), valid.Message());
    }
    return Astra::Core::Result<TimelineAsset>::Success(std::move(timeline));
}

Astra::Core::Result<void> ValidateTimeline(const TimelineAsset& timeline, Astra::Core::DiagnosticSink& diagnostics) {
    bool valid = true;
    if (timeline.id.path.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_TIMELINE_ID_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Timeline id is required."));
        valid = false;
    }
    for (const auto& track : timeline.tracks) {
        if (track.id.empty() || track.type.empty()) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_TIMELINE_TRACK_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Timeline track requires id and type."));
            valid = false;
        }
        double previous = -1.0;
        for (const auto& key : track.keys) {
            if (key.time_seconds < previous) {
                diagnostics.Emit(MakeDiagnostic("ASTRA_TIMELINE_KEY_ORDER_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Timeline keyframes must be sorted."));
                valid = false;
            }
            previous = key.time_seconds;
        }
    }
    return valid ? Astra::Core::Result<void>::Success() : Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "invalid timeline");
}

TimelineState EvaluateTimeline(const TimelineAsset& timeline, Astra::Core::u64 cursor_time_ns) {
    TimelineState state;
    state.timeline_id = timeline.id;
    state.cursor_time_ns = cursor_time_ns;
    const auto seconds = CursorSeconds(cursor_time_ns);
    for (const auto& track : timeline.tracks) {
        state.active_tracks.push_back(track.id);
        if (track.type == "camera") {
            for (auto key = track.keys.rbegin(); key != track.keys.rend(); ++key) {
                if (key->time_seconds <= seconds) {
                    state.camera = key->value;
                    state.camera["track"] = track.id;
                    state.camera["easing"] = key->easing;
                    break;
                }
            }
        }
        for (const auto& command : track.audio_events) {
            state.pending_events.push_back(command.command_id);
        }
        if (track.filter_profile.has_value()) {
            state.pending_events.push_back(track.filter_profile->id.ToString());
        }
    }
    std::ranges::sort(state.active_tracks);
    std::ranges::sort(state.pending_events);
    return state;
}

} // namespace Astra::Media
