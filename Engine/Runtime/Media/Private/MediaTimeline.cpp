#include <Astra/Media/Media.hpp>

#include "MediaInternal.hpp"

#include <algorithm>
#include <ranges>

namespace Astra::Media {

namespace {

Astra::Core::Diagnostic MakeDiagnostic(std::string code, Astra::Core::DiagnosticSeverity severity,
                                       std::string message) {
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

FilterGraphExecution ExecuteFilterGraphHeadless(const FilterProfile& profile,
                                                const RenderGraph& graph) {
    FilterGraphExecution execution;
    execution.provider_id = "astra.filter_graph.bgfx";
    execution.execution_mode = "headless_hash_fallback";
    execution.applications = ApplyFilterProfile(profile);
    nlohmann::json json = nlohmann::json::array();
    for (const auto& application : execution.applications) {
        json.push_back({{"pass_id", application.pass_id},
                        {"filter", application.filter},
                        {"target", application.target_name},
                        {"params_hash", application.params_hash}});
    }
    execution.output_hash = Private::StableHash(json.dump() + ToJson(graph).dump());
    return execution;
}

FilterGraphExecution ExecuteFilterGraphCpu(const FilterProfile& profile, const RenderGraph& graph,
                                           DecodedCpuBuffer* buffer,
                                           Astra::Core::DiagnosticSink& diagnostics) {
    auto execution = ExecuteFilterGraphHeadless(profile, graph);
    execution.execution_mode = "cpu_rgba";
    if (buffer == nullptr || buffer->pixels.empty() || buffer->format != "rgba8") {
        diagnostics.Emit(MakeDiagnostic("ASTRA_FILTER_GRAPH_CPU_BUFFER_INVALID",
                                        Astra::Core::DiagnosticSeverity::Blocking,
                                        "CPU FilterGraph execution requires an rgba8 buffer."));
        execution.output_hash = Private::StableHash(ToJson(execution).dump());
        return execution;
    }
    auto clamp_byte = [](double value) {
        return static_cast<Astra::Core::u8>(std::max(0.0, std::min(255.0, value)));
    };
    std::vector<Astra::Core::u8> source;
    for (const auto& pass : profile.passes) {
        if (pass.filter == "tint") {
            const auto r = pass.params.value("r", 1.0);
            const auto g = pass.params.value("g", 1.0);
            const auto b = pass.params.value("b", 1.0);
            for (std::size_t index = 0; index + 3 < buffer->pixels.size(); index += 4) {
                buffer->pixels[index + 0] = clamp_byte(buffer->pixels[index + 0] * r);
                buffer->pixels[index + 1] = clamp_byte(buffer->pixels[index + 1] * g);
                buffer->pixels[index + 2] = clamp_byte(buffer->pixels[index + 2] * b);
            }
        } else if (pass.filter == "grayscale") {
            for (std::size_t index = 0; index + 3 < buffer->pixels.size(); index += 4) {
                const auto gray = clamp_byte(buffer->pixels[index + 0] * 0.299 +
                                             buffer->pixels[index + 1] * 0.587 +
                                             buffer->pixels[index + 2] * 0.114);
                buffer->pixels[index + 0] = gray;
                buffer->pixels[index + 1] = gray;
                buffer->pixels[index + 2] = gray;
            }
        } else if (pass.filter == "opacity") {
            const auto alpha = pass.params.value("alpha", 1.0);
            for (std::size_t index = 0; index + 3 < buffer->pixels.size(); index += 4) {
                buffer->pixels[index + 3] = clamp_byte(buffer->pixels[index + 3] * alpha);
            }
        } else if (pass.filter == "blur3x3") {
            source = buffer->pixels;
            for (Astra::Core::u32 y = 0; y < buffer->height; ++y) {
                for (Astra::Core::u32 x = 0; x < buffer->width; ++x) {
                    double sum[4] = {0, 0, 0, 0};
                    double count = 0;
                    for (int oy = -1; oy <= 1; ++oy) {
                        for (int ox = -1; ox <= 1; ++ox) {
                            const auto sx = static_cast<int>(x) + ox;
                            const auto sy = static_cast<int>(y) + oy;
                            if (sx < 0 || sy < 0 || sx >= static_cast<int>(buffer->width) ||
                                sy >= static_cast<int>(buffer->height)) {
                                continue;
                            }
                            const auto offset = static_cast<std::size_t>(sy) * buffer->row_stride +
                                                static_cast<std::size_t>(sx) * 4;
                            for (int c = 0; c < 4; ++c) {
                                sum[c] += source[offset + c];
                            }
                            count += 1.0;
                        }
                    }
                    const auto out = static_cast<std::size_t>(y) * buffer->row_stride +
                                     static_cast<std::size_t>(x) * 4;
                    for (int c = 0; c < 4; ++c) {
                        buffer->pixels[out + c] = clamp_byte(sum[c] / count);
                    }
                }
            }
        } else {
            auto diagnostic = MakeDiagnostic("ASTRA_FILTER_GRAPH_UNSUPPORTED_FILTER",
                                             Astra::Core::DiagnosticSeverity::Blocking,
                                             "Unsupported CPU FilterGraph pass.");
            diagnostic.objects = {{"filter_pass", pass.id}, {"filter", pass.filter}};
            diagnostics.Emit(std::move(diagnostic));
        }
    }
    execution.output_hash = Private::StableHash(
        ToJson(execution).dump() +
        std::string(reinterpret_cast<const char*>(buffer->pixels.data()), buffer->pixels.size()));
    return execution;
}

DriverDiffReport CompareDriverCaptures(const FrameCapture& reference, const FrameCapture& candidate,
                                       const MediaBackendCapabilityReport& capabilities,
                                       bool require_driver_hardening) {
    DriverDiffReport report;
    report.reference_provider = reference.commands.value("provider", "astra.renderer2d.headless");
    report.candidate_provider = candidate.commands.value("provider", "astra.renderer2d.bgfx");
    report.render_hash_reference = reference.render_hash;
    report.render_hash_candidate = candidate.render_hash;
    report.text_hash_reference = reference.text_hash;
    report.text_hash_candidate = candidate.text_hash;
    report.audio_hash_reference = reference.audio_hash;
    report.audio_hash_candidate = candidate.audio_hash;
    report.filter_hash_reference = reference.filter_hash;
    report.filter_hash_candidate = candidate.filter_hash;
    if (require_driver_hardening && !capabilities.renderer2d_ready) {
        report.missing_required_capabilities.push_back("bgfx");
    }
    if (require_driver_hardening && !capabilities.ui_text_raster_ready) {
        report.missing_required_capabilities.push_back("skia");
    }
    if (require_driver_hardening && !capabilities.audio_mixer_ready) {
        report.missing_required_capabilities.push_back("miniaudio");
    }
    report.passed = report.missing_required_capabilities.empty() &&
                    !report.render_hash_reference.empty() &&
                    !report.render_hash_candidate.empty() &&
                    report.text_hash_reference == report.text_hash_candidate &&
                    report.audio_hash_reference == report.audio_hash_candidate &&
                    report.filter_hash_reference == report.filter_hash_candidate;
    return report;
}

Astra::Core::Result<TimelineAsset> TimelineFromJson(const nlohmann::json& json,
                                                    Astra::Core::DiagnosticSink& diagnostics) {
    auto id = Astra::Asset::ParseAssetUri(json.value("id", ""));
    if (!id) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_TIMELINE_ID_INVALID",
                                        Astra::Core::DiagnosticSeverity::Blocking,
                                        "Timeline id is invalid."));
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
            track.keys.push_back({key_json.value("t", 0.0),
                                  key_json.value("value", nlohmann::json::object()),
                                  key_json.value("easing", "linear")});
        }
        for (const auto& audio_json : track_json.value("audio_events", nlohmann::json::array())) {
            auto asset = Astra::Asset::ParseAssetUri(audio_json.value("asset", ""));
            if (!asset) {
                diagnostics.Emit(MakeDiagnostic("ASTRA_TIMELINE_AUDIO_ASSET_INVALID",
                                                Astra::Core::DiagnosticSeverity::Blocking,
                                                "Timeline audio event has invalid asset."));
                continue;
            }
            track.audio_events.push_back(
                {audio_json.value("command_id", track.id), audio_json.value("kind", "play"),
                 asset.Value(), audio_json.value("bus", "music"), audio_json.value("volume", 1.0),
                 audio_json.value("loop", false)});
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

Astra::Core::Result<void> ValidateTimeline(const TimelineAsset& timeline,
                                           Astra::Core::DiagnosticSink& diagnostics) {
    bool valid = true;
    if (timeline.id.path.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_TIMELINE_ID_MISSING",
                                        Astra::Core::DiagnosticSeverity::Blocking,
                                        "Timeline id is required."));
        valid = false;
    }
    for (const auto& track : timeline.tracks) {
        if (track.id.empty() || track.type.empty()) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_TIMELINE_TRACK_INVALID",
                                            Astra::Core::DiagnosticSeverity::Blocking,
                                            "Timeline track requires id and type."));
            valid = false;
        }
        double previous = -1.0;
        for (const auto& key : track.keys) {
            if (key.time_seconds < previous) {
                diagnostics.Emit(MakeDiagnostic("ASTRA_TIMELINE_KEY_ORDER_INVALID",
                                                Astra::Core::DiagnosticSeverity::Blocking,
                                                "Timeline keyframes must be sorted."));
                valid = false;
            }
            previous = key.time_seconds;
        }
    }
    return valid ? Astra::Core::Result<void>::Success()
                 : Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat,
                                                      "invalid timeline");
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
