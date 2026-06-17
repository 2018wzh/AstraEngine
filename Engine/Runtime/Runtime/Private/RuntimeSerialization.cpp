#include <Astra/Runtime/Runtime.hpp>

namespace Astra::Runtime {

namespace {

DirectorState DirectorFromJson(const nlohmann::json& json) {
    DirectorState state;
    state.phase = json.value("phase", "runtime");
    state.timeline_locked = json.value("timeline_locked", false);
    state.choice_locked = json.value("choice_locked", false);
    state.ai_permission_window = json.value("ai_permission_window", false);
    state.player_input_window = json.value("player_input_window", "closed");
    state.arbitration_log = json.value("arbitration_log", nlohmann::json::array());
    return state;
}

std::string TaskStateToString(RuntimeTaskState state) {
    switch (state) {
    case RuntimeTaskState::Pending:
        return "pending";
    case RuntimeTaskState::Running:
        return "running";
    case RuntimeTaskState::Waiting:
        return "waiting";
    case RuntimeTaskState::Cancelled:
        return "cancelled";
    case RuntimeTaskState::Completed:
        return "completed";
    }
    return "pending";
}

std::string WaitKindToString(RuntimeWaitKind kind) {
    switch (kind) {
    case RuntimeWaitKind::None:
        return "none";
    case RuntimeWaitKind::Event:
        return "event";
    case RuntimeWaitKind::Time:
        return "time";
    case RuntimeWaitKind::Asset:
        return "asset";
    case RuntimeWaitKind::Script:
        return "script";
    case RuntimeWaitKind::Debugger:
        return "debugger";
    }
    return "none";
}

RuntimeTaskState TaskStateFromString(std::string_view value) {
    if (value == "running") {
        return RuntimeTaskState::Running;
    }
    if (value == "waiting") {
        return RuntimeTaskState::Waiting;
    }
    if (value == "cancelled") {
        return RuntimeTaskState::Cancelled;
    }
    if (value == "completed") {
        return RuntimeTaskState::Completed;
    }
    return RuntimeTaskState::Pending;
}

RuntimeWaitKind WaitKindFromString(std::string_view value) {
    if (value == "event") {
        return RuntimeWaitKind::Event;
    }
    if (value == "time") {
        return RuntimeWaitKind::Time;
    }
    if (value == "asset") {
        return RuntimeWaitKind::Asset;
    }
    if (value == "script") {
        return RuntimeWaitKind::Script;
    }
    if (value == "debugger") {
        return RuntimeWaitKind::Debugger;
    }
    return RuntimeWaitKind::None;
}

RuntimeWaitCondition RuntimeWaitFromJson(const nlohmann::json& json) {
    RuntimeWaitCondition wait;
    wait.kind = WaitKindFromString(json.value("kind", "none"));
    if (json.contains("event_type") && !json.value("event_type", "").empty()) {
        auto event_type = Astra::Core::ParseStableId(json.value("event_type", ""));
        if (event_type) {
            wait.event_type = event_type.Value();
        }
    }
    wait.asset = json.value("asset", "");
    wait.wake_frame = json.value("wake_frame", 0ull);
    wait.script_state = json.value("script_state", "");
    return wait;
}

Astra::Core::Result<RuntimeTask> RuntimeTaskFromJson(const nlohmann::json& json) {
    RuntimeTask task;
    auto id = Astra::Core::ParseStableId(json.value("id", ""));
    if (!id) {
        return Astra::Core::Result<RuntimeTask>::Failure(Astra::Core::ErrorCode::InvalidFormat, "runtime task has invalid id");
    }
    task.id = id.Value();
    task.state = TaskStateFromString(json.value("state", "pending"));
    task.owner = {json.value("owner", nlohmann::json::object()).value("kind", ""), json.value("owner", nlohmann::json::object()).value("id", "")};
    task.wait = RuntimeWaitFromJson(json.value("wait", nlohmann::json::object()));
    task.continuation = json.value("continuation", nlohmann::json::object());
    task.cancellation_policy = json.value("cancellation_policy", "cancel_on_owner_destroy");
    if (json.contains("emit_on_wake") && !json.at("emit_on_wake").value("type", "").empty()) {
        auto event = RuntimeEventFromJson(json.at("emit_on_wake"));
        if (event) {
            task.emit_on_wake = event.Value();
        }
    }
    return Astra::Core::Result<RuntimeTask>::Success(std::move(task));
}

} // namespace

nlohmann::json ToJson(const RuntimeEventEndpoint& endpoint) {
    return {{"kind", endpoint.kind}, {"id", endpoint.id}};
}

nlohmann::json ToJson(const RuntimeEventTrace& trace) {
    return {{"script_location", trace.script_location}, {"audit_ref", trace.audit_ref}};
}

nlohmann::json ToJson(const RuntimeEvent& event) {
    return {
        {"event_id", event.event_id.ToString()},
        {"type", event.type.ToString()},
        {"category", event.category},
        {"priority", event.priority},
        {"sequence", event.sequence},
        {"frame_index", event.frame_index},
        {"source", ToJson(event.source)},
        {"target", ToJson(event.target)},
        {"payload_schema", event.payload_schema},
        {"payload", event.payload},
        {"trace", ToJson(event.trace)},
    };
}

nlohmann::json ToJson(const RuntimeEventSubscription& subscription) {
    return {
        {"id", subscription.id.ToString()},
        {"event_type", subscription.event_type.ToString()},
        {"owner", subscription.owner},
        {"active", subscription.active},
    };
}

nlohmann::json ToJson(const DirectorState& state) {
    return {{"phase", state.phase}, {"timeline_locked", state.timeline_locked}, {"choice_locked", state.choice_locked}, {"ai_permission_window", state.ai_permission_window}, {"player_input_window", state.player_input_window}, {"arbitration_log", state.arbitration_log}};
}

nlohmann::json ToJson(const ControlPolicyResult& result) {
    std::string decision = "allow";
    if (result.decision == ControlDecision::Queue) {
        decision = "queue";
    } else if (result.decision == ControlDecision::Reject) {
        decision = "reject";
    }
    return {{"decision", decision}, {"reason", result.reason}};
}

nlohmann::json ToJson(const RuntimeWaitCondition& wait) {
    return {
        {"kind", WaitKindToString(wait.kind)},
        {"event_type", wait.event_type.ToString()},
        {"asset", wait.asset},
        {"wake_frame", wait.wake_frame},
        {"script_state", wait.script_state},
    };
}

nlohmann::json ToJson(const RuntimeTask& task) {
    return {
        {"id", task.id.ToString()},
        {"state", TaskStateToString(task.state)},
        {"owner", ToJson(task.owner)},
        {"wait", ToJson(task.wait)},
        {"continuation", task.continuation},
        {"emit_on_wake", ToJson(task.emit_on_wake)},
        {"cancellation_policy", task.cancellation_policy},
    };
}

nlohmann::json ToJson(const SchedulerSnapshot& scheduler) {
    nlohmann::json tasks = nlohmann::json::array();
    for (const auto& task : scheduler.tasks) {
        tasks.push_back(ToJson(task));
    }
    return {{"schema", scheduler.schema}, {"tasks", tasks}};
}

nlohmann::json ToJson(const RuntimeHashes& hashes) {
    return {{"state_hash", hashes.state_hash}, {"event_hash", hashes.event_hash}, {"presentation_hash", hashes.presentation_hash}};
}

nlohmann::json ToJson(const RuntimeReplay& replay) {
    nlohmann::json checkpoints = nlohmann::json::array();
    for (const auto& checkpoint : replay.checkpoints) {
        checkpoints.push_back({{"frame_index", checkpoint.frame_index}, {"hashes", ToJson(checkpoint.hashes)}});
    }
    return {{"schema", replay.schema}, {"version", replay.version}, {"random_seed", replay.random_seed}, {"inputs", replay.inputs}, {"script_decisions", replay.script_decisions}, {"choice_selections", replay.choice_selections}, {"committed_ai_output", replay.committed_ai_output}, {"events", replay.events}, {"checkpoints", checkpoints}, {"hashes", ToJson(replay.hashes)}};
}

nlohmann::json ToJson(const SaveContainer& container) {
    return {
        {"schema", container.schema},
        {"version", container.version},
        {"engine_version", container.engine_version},
        {"package_hash", container.package_hash},
        {"created_frame", container.created_frame},
        {"runtime_snapshot", ToJson(container.runtime_snapshot)},
        {"script_snapshot", container.script_snapshot},
        {"media_state", container.media_state},
    };
}

nlohmann::json ToJson(const SaveSection& section) {
    return {
        {"name", section.name},
        {"schema", section.schema},
        {"version", section.version},
        {"hash", section.hash},
        {"compressed", section.compressed},
        {"payload", section.payload},
        {"compressed_payload", section.compressed_payload},
    };
}

nlohmann::json ToJson(const SaveContainerV2& container) {
    nlohmann::json sections = nlohmann::json::array();
    for (const auto& section : container.sections) {
        sections.push_back(ToJson(section));
    }
    return {{"schema", container.schema}, {"version", container.version}, {"header", container.header}, {"sections", sections}};
}

nlohmann::json ToJson(const ReplayComparisonReport& report) {
    nlohmann::json localized = nlohmann::json::array();
    for (const auto& mismatch : report.localized_mismatches) {
        localized.push_back({
            {"kind", mismatch.kind},
            {"frame_index", mismatch.frame_index},
            {"event_sequence", mismatch.event_sequence},
            {"actor", mismatch.actor},
            {"component", mismatch.component},
            {"script_location", mismatch.script_location},
            {"provider_id", mismatch.provider_id},
            {"asset_or_pass", mismatch.asset_or_pass},
            {"expected", mismatch.expected},
            {"actual", mismatch.actual},
        });
    }
    return {
        {"schema", report.schema},
        {"passed", report.passed},
        {"expected", ToJson(report.expected)},
        {"actual", ToJson(report.actual)},
        {"mismatches", report.mismatches},
        {"localized_mismatches", localized},
    };
}

nlohmann::json ToJson(const RuntimeSnapshot& snapshot) {
    return {
        {"schema", snapshot.schema},
        {"version", snapshot.version},
        {"frame_index", snapshot.frame_index},
        {"fixed_step_index", snapshot.fixed_step_index},
        {"next_event_sequence", snapshot.next_event_sequence},
        {"random_seed", snapshot.random_seed},
        {"world", Astra::Scene::ToJson(snapshot.world)},
        {"director", ToJson(snapshot.director)},
        {"scheduler", ToJson(snapshot.scheduler)},
        {"replay_events", snapshot.replay_events},
        {"hashes", ToJson(snapshot.hashes)},
    };
}

ReplayComparisonReport CompareReplayHashes(const RuntimeHashes& expected, const RuntimeHashes& actual) {
    ReplayComparisonReport report;
    report.expected = expected;
    report.actual = actual;
    if (expected.state_hash != actual.state_hash) {
        report.mismatches.push_back("state_hash");
        report.localized_mismatches.push_back({"state_hash", 0, 0, "", "", "", "", "", expected.state_hash, actual.state_hash});
    }
    if (expected.event_hash != actual.event_hash) {
        report.mismatches.push_back("event_hash");
        report.localized_mismatches.push_back({"event_hash", 0, 0, "", "", "", "", "", expected.event_hash, actual.event_hash});
    }
    if (expected.presentation_hash != actual.presentation_hash) {
        report.mismatches.push_back("presentation_hash");
        report.localized_mismatches.push_back({"presentation_hash", 0, 0, "", "", "", "astra.media", "presentation", expected.presentation_hash, actual.presentation_hash});
    }
    report.passed = report.mismatches.empty();
    return report;
}

Astra::Core::Result<RuntimeEvent> RuntimeEventFromJson(const nlohmann::json& json) {
    auto event_id = Astra::Core::ParseStableId(json.at("event_id").get<std::string>());
    auto type = Astra::Core::ParseStableId(json.at("type").get<std::string>());
    if (!event_id || !type) {
        return Astra::Core::Result<RuntimeEvent>::Failure(Astra::Core::ErrorCode::InvalidFormat, "runtime event has invalid stable id");
    }
    RuntimeEvent event;
    event.event_id = event_id.Value();
    event.type = type.Value();
    event.category = json.value("category", "runtime");
    event.priority = json.value("priority", 0u);
    event.sequence = json.value("sequence", 0ull);
    event.frame_index = json.value("frame_index", 0ull);
    event.source = {json.value("source", nlohmann::json::object()).value("kind", ""), json.value("source", nlohmann::json::object()).value("id", "")};
    event.target = {json.value("target", nlohmann::json::object()).value("kind", ""), json.value("target", nlohmann::json::object()).value("id", "")};
    event.payload_schema = json.value("payload_schema", "");
    event.payload = json.value("payload", nlohmann::json::object());
    event.trace = {json.value("trace", nlohmann::json::object()).value("script_location", ""), json.value("trace", nlohmann::json::object()).value("audit_ref", "")};
    return Astra::Core::Result<RuntimeEvent>::Success(std::move(event));
}

Astra::Core::Result<RuntimeSnapshot> RuntimeSnapshotFromJson(const nlohmann::json& json) {
    if (json.value("schema", std::string()) != SnapshotSchema) {
        return Astra::Core::Result<RuntimeSnapshot>::Failure(Astra::Core::ErrorCode::InvalidFormat, "unsupported runtime snapshot schema");
    }
    auto world = Astra::Scene::WorldSnapshotFromJson(json.at("world"));
    if (!world) {
        return Astra::Core::Result<RuntimeSnapshot>::Failure(world.Error(), world.Message());
    }
    RuntimeSnapshot snapshot;
    snapshot.version = json.value("version", 1u);
    snapshot.frame_index = json.value("frame_index", 0ull);
    snapshot.fixed_step_index = json.value("fixed_step_index", 0ull);
    snapshot.next_event_sequence = json.value("next_event_sequence", 1ull);
    snapshot.random_seed = json.value("random_seed", 0ull);
    snapshot.world = world.Value();
    snapshot.director = DirectorFromJson(json.value("director", nlohmann::json::object()));
    if (json.contains("scheduler")) {
        snapshot.scheduler.schema = json.at("scheduler").value("schema", "astra.runtime.scheduler.v1");
        for (const auto& task_json : json.at("scheduler").value("tasks", nlohmann::json::array())) {
            auto task = RuntimeTaskFromJson(task_json);
            if (task) {
                snapshot.scheduler.tasks.push_back(task.Value());
            }
        }
    }
    snapshot.replay_events = json.value("replay_events", nlohmann::json::array());
    if (json.contains("hashes")) {
        snapshot.hashes = {
            json.at("hashes").value("state_hash", ""),
            json.at("hashes").value("event_hash", ""),
            json.at("hashes").value("presentation_hash", ""),
        };
    }
    return Astra::Core::Result<RuntimeSnapshot>::Success(std::move(snapshot));
}

Astra::Core::Result<SaveContainerV2> SaveContainerV2FromJson(const nlohmann::json& json) {
    if (json.value("schema", std::string()) != SaveContainerV2Schema) {
        return Astra::Core::Result<SaveContainerV2>::Failure(Astra::Core::ErrorCode::InvalidFormat, "unsupported runtime save container schema");
    }
    SaveContainerV2 container;
    container.version = json.value("version", 2u);
    container.header = json.value("header", nlohmann::json::object());
    for (const auto& section_json : json.value("sections", nlohmann::json::array())) {
        SaveSection section;
        section.name = section_json.value("name", "");
        section.schema = section_json.value("schema", "");
        section.version = section_json.value("version", 1u);
        section.hash = section_json.value("hash", "");
        section.compressed = section_json.value("compressed", false);
        section.payload = section_json.value("payload", nlohmann::json::object());
        section.compressed_payload = section_json.value("compressed_payload", std::vector<Astra::Core::u8>{});
        container.sections.push_back(std::move(section));
    }
    return Astra::Core::Result<SaveContainerV2>::Success(std::move(container));
}

} // namespace Astra::Runtime
