#include <Astra/Runtime/Runtime.hpp>

#include <algorithm>
#include <functional>
#include <sstream>
#include <unordered_map>

namespace Astra::Runtime {

namespace {

std::string HexHash(Astra::Core::u64 value) {
    std::ostringstream output;
    output << std::hex << value;
    return output.str();
}

std::string StableHash(std::string_view text) {
    constexpr Astra::Core::u64 offset = 14695981039346656037ull;
    constexpr Astra::Core::u64 prime = 1099511628211ull;
    Astra::Core::u64 value = offset;
    for (const auto character : text) {
        value ^= static_cast<unsigned char>(character);
        value *= prime;
    }
    return HexHash(value);
}

std::string EventModeToString(RuntimeEventMode mode) {
    switch (mode) {
    case RuntimeEventMode::Immediate:
        return "immediate";
    case RuntimeEventMode::Queued:
        return "queued";
    case RuntimeEventMode::Deferred:
        return "deferred";
    }
    return "queued";
}

DirectorState DirectorFromJson(const nlohmann::json& json) {
    DirectorState state;
    state.phase = json.value("phase", "runtime");
    state.timeline_locked = json.value("timeline_locked", false);
    state.choice_locked = json.value("choice_locked", false);
    state.ai_permission_window = json.value("ai_permission_window", false);
    return state;
}

} // namespace

class RuntimeWorld::Impl {
public:
    explicit Impl(Astra::Core::u64 seed) : random_seed(seed) {}

    Astra::Scene::ActorWorld scene;
    RuntimeEventBus events;
    DirectorState director;
    Astra::Core::u64 frame_index = 0;
    Astra::Core::u64 fixed_step_index = 0;
    Astra::Core::u64 next_event_sequence = 1;
    Astra::Core::u64 random_seed = 0;
    std::unordered_map<std::string, StateMachineDefinition> definitions;

    void ApplyStateMachines(const RuntimeEvent& event) {
        auto snapshot = scene.Snapshot();
        for (const auto& actor : snapshot.actors) {
            Astra::Scene::ActorHandle handle{actor.id, actor.generation};
            auto state_machine = scene.FindComponent(handle, "astra.state_machine");
            if (!state_machine) {
                continue;
            }
            const auto definition_id = state_machine->data.value("state_machine_id", "");
            const auto definition_it = definitions.find(definition_id);
            if (definition_it == definitions.end()) {
                continue;
            }
            const auto current_state = state_machine->data.value("current_state", definition_it->second.initial_state);
            for (const auto& transition : definition_it->second.transitions) {
                if (transition.from_state == current_state && transition.event_type == event.type) {
                    state_machine->data["current_state"] = transition.to_state;
                    Astra::Core::DiagnosticSink ignored;
                    (void)scene.AddOrReplaceComponent(handle, *state_machine, ignored);
                    break;
                }
            }
        }
    }
};

void RuntimeEventBus::Emit(RuntimeEvent event, RuntimeEventMode mode) {
    trace_.push_back(event);
    if (mode == RuntimeEventMode::Queued) {
        queued_.push_back(std::move(event));
    } else if (mode == RuntimeEventMode::Deferred) {
        deferred_.push_back(std::move(event));
    }
}

std::vector<RuntimeEvent> RuntimeEventBus::DrainQueued() {
    std::vector<RuntimeEvent> events;
    while (!queued_.empty()) {
        events.push_back(std::move(queued_.front()));
        queued_.pop_front();
    }
    std::ranges::sort(events, {}, [](const RuntimeEvent& event) { return event.sequence; });
    return events;
}

void RuntimeEventBus::AdvanceDeferred() {
    while (!deferred_.empty()) {
        queued_.push_back(std::move(deferred_.front()));
        deferred_.pop_front();
    }
}

std::size_t RuntimeEventBus::QueuedCount() const {
    return queued_.size();
}

const std::vector<RuntimeEvent>& RuntimeEventBus::Trace() const {
    return trace_;
}

void RuntimeEventBus::RestoreTrace(std::vector<RuntimeEvent> trace) {
    trace_ = std::move(trace);
}

void RuntimeEventBus::Clear() {
    queued_.clear();
    deferred_.clear();
    trace_.clear();
}

RuntimeWorld::RuntimeWorld(Astra::Core::u64 random_seed) : impl_(std::make_unique<Impl>(random_seed)) {}
RuntimeWorld::RuntimeWorld(RuntimeWorld&&) noexcept = default;
RuntimeWorld& RuntimeWorld::operator=(RuntimeWorld&&) noexcept = default;
RuntimeWorld::~RuntimeWorld() = default;

Astra::Scene::ActorWorld& RuntimeWorld::Scene() {
    return impl_->scene;
}

const Astra::Scene::ActorWorld& RuntimeWorld::Scene() const {
    return impl_->scene;
}

RuntimeEventBus& RuntimeWorld::Events() {
    return impl_->events;
}

const RuntimeEventBus& RuntimeWorld::Events() const {
    return impl_->events;
}

const DirectorState& RuntimeWorld::Director() const {
    return impl_->director;
}

void RuntimeWorld::SetDirector(DirectorState state) {
    impl_->director = std::move(state);
}

void RuntimeWorld::RegisterStateMachine(StateMachineDefinition definition) {
    impl_->definitions[definition.id.ToString()] = std::move(definition);
}

ControlPolicyResult RuntimeWorld::EvaluateControlPolicy(const ControlPolicyRequest& request, Astra::Core::DiagnosticSink& diagnostics) const {
    auto handle = impl_->scene.ResolveActor(request.actor_id, diagnostics);
    if (!handle) {
        return {ControlDecision::Reject, "missing actor"};
    }
    auto policy = impl_->scene.FindComponent(handle.value(), "astra.control_policy");
    if (!policy) {
        return {ControlDecision::Allow, "no policy component"};
    }
    const auto locked_channels = policy->data.value("locked_channels", nlohmann::json::array());
    const auto channel_locked = std::ranges::any_of(locked_channels, [&](const nlohmann::json& value) { return value.get<std::string>() == request.channel; });
    if (!channel_locked) {
        return {ControlDecision::Allow, "channel unlocked"};
    }
    const auto owner = policy->data.value("owner", "system");
    if (owner == request.requester) {
        return {ControlDecision::Allow, "requester owns lock"};
    }
    if (request.priority >= 100) {
        return {ControlDecision::Allow, "system priority override"};
    }
    const auto queue_locked = policy->data.value("queue_locked", true);
    return queue_locked ? ControlPolicyResult{ControlDecision::Queue, "locked channel queues lower priority request"} : ControlPolicyResult{ControlDecision::Reject, "locked channel rejects lower priority request"};
}

Astra::Core::Result<void> RuntimeWorld::Emit(RuntimeEvent event, RuntimeEventMode mode, Astra::Core::DiagnosticSink&) {
    if (event.sequence == 0) {
        event.sequence = impl_->next_event_sequence++;
    } else {
        impl_->next_event_sequence = std::max(impl_->next_event_sequence, event.sequence + 1);
    }
    event.frame_index = impl_->frame_index;
    if (event.event_id.Empty()) {
        event.event_id = Astra::Core::StableId(Astra::Core::StableIdKind::EventType, "runtime/" + std::to_string(event.sequence));
    }
    event.trace.audit_ref = EventModeToString(mode);
    const auto copied = event;
    impl_->events.Emit(std::move(event), mode);
    if (mode == RuntimeEventMode::Immediate) {
        impl_->ApplyStateMachines(copied);
    }
    return Astra::Core::Result<void>::Success();
}

Astra::Core::Result<void> RuntimeWorld::Tick(Astra::Core::DiagnosticSink&) {
    impl_->events.AdvanceDeferred();
    const auto events = impl_->events.DrainQueued();
    for (const auto& event : events) {
        impl_->ApplyStateMachines(event);
    }
    impl_->frame_index += 1;
    impl_->fixed_step_index += 1;
    return Astra::Core::Result<void>::Success();
}

RuntimeSnapshot RuntimeWorld::CaptureSnapshot() const {
    RuntimeSnapshot snapshot;
    snapshot.frame_index = impl_->frame_index;
    snapshot.fixed_step_index = impl_->fixed_step_index;
    snapshot.next_event_sequence = impl_->next_event_sequence;
    snapshot.random_seed = impl_->random_seed;
    snapshot.world = impl_->scene.Snapshot();
    snapshot.director = impl_->director;
    for (const auto& event : impl_->events.Trace()) {
        snapshot.replay_events.push_back(ToJson(event));
    }
    snapshot.hashes = Hashes();
    return snapshot;
}

RuntimeReplay RuntimeWorld::CaptureReplay() const {
    RuntimeReplay replay;
    replay.random_seed = impl_->random_seed;
    for (const auto& event : impl_->events.Trace()) {
        replay.events.push_back(ToJson(event));
    }
    replay.hashes = Hashes();
    return replay;
}

Astra::Core::VersionedDocument RuntimeWorld::Save() const {
    return {SnapshotSchema, 1, "runtime:/world", ToJson(CaptureSnapshot())};
}

Astra::Core::Result<void> RuntimeWorld::Load(const Astra::Core::VersionedDocument& document, Astra::Core::DiagnosticSink& diagnostics) {
    if (document.schema != SnapshotSchema) {
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "unsupported runtime snapshot schema");
    }
    auto snapshot = RuntimeSnapshotFromJson(document.payload);
    if (!snapshot) {
        return Astra::Core::Result<void>::Failure(snapshot.Error(), snapshot.Message());
    }
    auto restored = impl_->scene.Restore(snapshot.Value().world, diagnostics);
    if (!restored) {
        return restored;
    }
    impl_->frame_index = snapshot.Value().frame_index;
    impl_->fixed_step_index = snapshot.Value().fixed_step_index;
    impl_->next_event_sequence = snapshot.Value().next_event_sequence;
    impl_->random_seed = snapshot.Value().random_seed;
    impl_->director = snapshot.Value().director;
    std::vector<RuntimeEvent> trace;
    for (const auto& event_json : snapshot.Value().replay_events) {
        auto event = RuntimeEventFromJson(event_json);
        if (event) {
            trace.push_back(event.Value());
        }
    }
    impl_->events.Clear();
    impl_->events.RestoreTrace(std::move(trace));
    return Astra::Core::Result<void>::Success();
}

RuntimeHashes RuntimeWorld::Hashes() const {
    const auto world_json = ToJson(impl_->scene.Snapshot()).dump();
    nlohmann::json events = nlohmann::json::array();
    for (const auto& event : impl_->events.Trace()) {
        events.push_back(ToJson(event));
    }
    return {StableHash(world_json), StableHash(events.dump()), StableHash(world_json + events.dump())};
}

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
        {"sequence", event.sequence},
        {"frame_index", event.frame_index},
        {"source", ToJson(event.source)},
        {"target", ToJson(event.target)},
        {"payload_schema", event.payload_schema},
        {"payload", event.payload},
        {"trace", ToJson(event.trace)},
    };
}

nlohmann::json ToJson(const DirectorState& state) {
    return {{"phase", state.phase}, {"timeline_locked", state.timeline_locked}, {"choice_locked", state.choice_locked}, {"ai_permission_window", state.ai_permission_window}};
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

nlohmann::json ToJson(const RuntimeHashes& hashes) {
    return {{"state_hash", hashes.state_hash}, {"event_hash", hashes.event_hash}, {"presentation_hash", hashes.presentation_hash}};
}

nlohmann::json ToJson(const RuntimeReplay& replay) {
    return {{"schema", replay.schema}, {"version", replay.version}, {"random_seed", replay.random_seed}, {"events", replay.events}, {"hashes", ToJson(replay.hashes)}};
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
        {"replay_events", snapshot.replay_events},
        {"hashes", ToJson(snapshot.hashes)},
    };
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

} // namespace Astra::Runtime
