#include <Astra/Runtime/Runtime.hpp>

#include <algorithm>
#include <functional>
#include <map>
#include <sstream>
#include <unordered_map>
#include <zstd.h>

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

std::vector<Astra::Core::u8> CompressText(std::string_view text) {
    std::vector<Astra::Core::u8> output(ZSTD_compressBound(text.size()));
    const auto size = ZSTD_compress(output.data(), output.size(), text.data(), text.size(), 1);
    if (ZSTD_isError(size)) {
        return {};
    }
    output.resize(size);
    return output;
}

Astra::Core::Result<std::string> DecompressText(const std::vector<Astra::Core::u8>& compressed) {
    const auto expected_size = ZSTD_getFrameContentSize(compressed.data(), compressed.size());
    if (expected_size == ZSTD_CONTENTSIZE_ERROR || expected_size == ZSTD_CONTENTSIZE_UNKNOWN) {
        return Astra::Core::Result<std::string>::Failure(Astra::Core::ErrorCode::InvalidFormat, "invalid zstd save section");
    }
    std::string output(static_cast<std::size_t>(expected_size), '\0');
    const auto size = ZSTD_decompress(output.data(), output.size(), compressed.data(), compressed.size());
    if (ZSTD_isError(size)) {
        return Astra::Core::Result<std::string>::Failure(Astra::Core::ErrorCode::InvalidFormat, "could not decompress zstd save section");
    }
    output.resize(size);
    return Astra::Core::Result<std::string>::Success(std::move(output));
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

class RuntimeWorld::Impl {
public:
    explicit Impl(Astra::Core::u64 seed) : random_seed(seed) {}

    Astra::Scene::ActorWorld scene;
    RuntimeEventBus events;
    DirectorState director;
    Astra::Core::u64 frame_index = 0;
    Astra::Core::u64 fixed_step_index = 0;
    Astra::Core::u64 next_event_sequence = 1;
    Astra::Core::u64 next_subscription_sequence = 1;
    Astra::Core::u64 next_task_sequence = 1;
    Astra::Core::u64 random_seed = 0;
    std::unordered_map<std::string, StateMachineDefinition> definitions;
    std::map<std::string, RuntimeEventSubscription> subscriptions;
    std::map<std::string, RuntimeTask> tasks;

    void ApplyStateMachineForActor(const RuntimeEvent& event, const Astra::Scene::ActorSnapshot& actor) {
            Astra::Scene::ActorHandle handle{actor.id, actor.generation};
            auto state_machine = scene.FindComponent(handle, "astra.state_machine");
            if (!state_machine) {
                return;
            }
            const auto definition_id = state_machine->data.value("state_machine_id", "");
            const auto definition_it = definitions.find(definition_id);
            if (definition_it == definitions.end()) {
                return;
            }
            const auto current_state = state_machine->data.value("current_state", definition_it->second.initial_state);
            for (const auto& transition : definition_it->second.transitions) {
                if (transition.from_state == current_state && transition.event_type == event.type) {
                    state_machine->data["current_state"] = transition.to_state;
                    state_machine->data["last_transition_event"] = event.event_id.ToString();
                    state_machine->data["last_transition_sequence"] = event.sequence;
                    state_machine->data["trace"].push_back({{"from", transition.from_state}, {"to", transition.to_state}, {"event", event.type.ToString()}});
                    Astra::Core::DiagnosticSink ignored;
                    (void)scene.AddOrReplaceComponent(handle, *state_machine, ignored);
                    break;
                }
            }
    }

    void ApplyStateMachines(const RuntimeEvent& event) {
        if (event.target.kind == "actor" && !event.target.id.empty()) {
            Astra::Core::DiagnosticSink ignored;
            auto actor_id = Astra::Core::ParseStableId(event.target.id);
            if (actor_id) {
                auto handle = scene.ResolveActor(actor_id.Value(), ignored);
                if (handle) {
                    auto actor = scene.SnapshotActor(handle.value());
                    if (actor) {
                        ApplyStateMachineForActor(event, actor.value());
                    }
                }
            }
            return;
        }
        auto snapshot = scene.Snapshot();
        for (const auto& actor : snapshot.actors) {
            ApplyStateMachineForActor(event, actor);
        }
    }

    void StepScheduler(const std::vector<RuntimeEvent>& drained, Astra::Core::DiagnosticSink& diagnostics) {
        std::vector<Astra::Core::EventTypeId> event_types;
        for (const auto& event : drained) {
            event_types.push_back(event.type);
        }
        for (auto& [task_key, task] : tasks) {
            (void)task_key;
            if (task.state == RuntimeTaskState::Cancelled || task.state == RuntimeTaskState::Completed) {
                continue;
            }
            if (task.owner.kind == "actor") {
                auto owner_id = Astra::Core::ParseStableId(task.owner.id);
                if (owner_id) {
                    auto owner = scene.ResolveActor(owner_id.Value(), diagnostics);
                    if (!owner && task.cancellation_policy == "cancel_on_owner_destroy") {
                        task.state = RuntimeTaskState::Cancelled;
                        task.continuation["cancel_reason"] = "owner_destroyed";
                        continue;
                    }
                }
            }
            bool wake = false;
            if (task.wait.kind == RuntimeWaitKind::None) {
                wake = true;
            } else if (task.wait.kind == RuntimeWaitKind::Time) {
                wake = task.wait.wake_frame <= fixed_step_index + 1;
            } else if (task.wait.kind == RuntimeWaitKind::Event) {
                wake = std::ranges::any_of(event_types, [&](const auto& type) { return type == task.wait.event_type; });
            } else if (task.wait.kind == RuntimeWaitKind::Asset) {
                wake = task.continuation.value("asset_ready", false);
            } else if (task.wait.kind == RuntimeWaitKind::Script) {
                wake = task.continuation.value("script_ready", false);
            }
            if (wake) {
                task.state = RuntimeTaskState::Completed;
                if (!task.emit_on_wake.type.Empty()) {
                    auto event = task.emit_on_wake;
                    if (event.sequence == 0) {
                        event.sequence = next_event_sequence++;
                    }
                    event.frame_index = frame_index;
                    if (event.event_id.Empty()) {
                        event.event_id = Astra::Core::StableId(Astra::Core::StableIdKind::EventType, "runtime/" + std::to_string(event.sequence));
                    }
                    event.trace.audit_ref = "scheduled";
                    events.Emit(std::move(event), RuntimeEventMode::Queued);
                }
            } else if (task.state == RuntimeTaskState::Pending) {
                task.state = RuntimeTaskState::Waiting;
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
    std::ranges::sort(events, [](const RuntimeEvent& lhs, const RuntimeEvent& rhs) {
        if (lhs.priority != rhs.priority) {
            return lhs.priority > rhs.priority;
        }
        if (lhs.sequence != rhs.sequence) {
            return lhs.sequence < rhs.sequence;
        }
        if (lhs.target.id != rhs.target.id) {
            return lhs.target.id < rhs.target.id;
        }
        return lhs.type.ToString() < rhs.type.ToString();
    });
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

RuntimeEventSubscription RuntimeWorld::Subscribe(Astra::Core::EventTypeId event_type, std::string owner) {
    RuntimeEventSubscription subscription;
    subscription.id = Astra::Core::StableId(Astra::Core::StableIdKind::Unknown, "runtime/subscription/" + std::to_string(impl_->next_subscription_sequence++));
    subscription.event_type = std::move(event_type);
    subscription.owner = std::move(owner);
    impl_->subscriptions[subscription.id.ToString()] = subscription;
    return subscription;
}

Astra::Core::Result<void> RuntimeWorld::Unsubscribe(const Astra::Core::StableId& subscription, Astra::Core::DiagnosticSink& diagnostics) {
    auto it = impl_->subscriptions.find(subscription.ToString());
    if (it == impl_->subscriptions.end()) {
        Astra::Core::Diagnostic diagnostic;
        diagnostic.code = "ASTRA_RUNTIME_SUBSCRIPTION_MISSING";
        diagnostic.category = "runtime.event";
        diagnostic.severity = Astra::Core::DiagnosticSeverity::Error;
        diagnostic.message = "Runtime event subscription is missing or already removed.";
        diagnostic.objects = {{"subscription", subscription.ToString()}};
        diagnostics.Emit(std::move(diagnostic));
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::NotFound, "subscription is missing");
    }
    impl_->subscriptions.erase(it);
    return Astra::Core::Result<void>::Success();
}

Astra::Core::Result<void> RuntimeWorld::ScheduleTask(RuntimeTask task, Astra::Core::DiagnosticSink&) {
    if (task.id.Empty()) {
        task.id = Astra::Core::StableId(Astra::Core::StableIdKind::Task, "runtime/" + std::to_string(impl_->next_task_sequence++));
    }
    if (task.state == RuntimeTaskState::Pending && task.wait.kind != RuntimeWaitKind::None) {
        task.state = RuntimeTaskState::Waiting;
    }
    impl_->tasks[task.id.ToString()] = std::move(task);
    return Astra::Core::Result<void>::Success();
}

Astra::Core::Result<void> RuntimeWorld::CancelTask(const Astra::Core::StableId& task_id, std::string reason, Astra::Core::DiagnosticSink& diagnostics) {
    auto it = impl_->tasks.find(task_id.ToString());
    if (it == impl_->tasks.end()) {
        Astra::Core::Diagnostic diagnostic;
        diagnostic.code = "ASTRA_RUNTIME_TASK_MISSING";
        diagnostic.category = "runtime.scheduler";
        diagnostic.severity = Astra::Core::DiagnosticSeverity::Error;
        diagnostic.message = "Runtime scheduler task is missing.";
        diagnostic.objects = {{"task", task_id.ToString()}};
        diagnostics.Emit(std::move(diagnostic));
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::NotFound, "task is missing");
    }
    it->second.state = RuntimeTaskState::Cancelled;
    it->second.continuation["cancel_reason"] = std::move(reason);
    return Astra::Core::Result<void>::Success();
}

SchedulerSnapshot RuntimeWorld::Scheduler() const {
    SchedulerSnapshot snapshot;
    for (const auto& [id, task] : impl_->tasks) {
        (void)id;
        snapshot.tasks.push_back(task);
    }
    return snapshot;
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
    const auto inherited_priority = policy->data.value("priority_inheritance", 0u);
    if (request.priority + inherited_priority >= 100) {
        return {ControlDecision::Allow, "system priority override"};
    }
    const auto interrupt_policy = policy->data.value("interrupt_policy", request.interrupt_policy);
    if (interrupt_policy == "reject" || interrupt_policy == "reject_lower_priority") {
        return {ControlDecision::Reject, "locked channel rejects lower priority request"};
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
    Astra::Core::DiagnosticSink ignored;
    impl_->StepScheduler(events, ignored);
    const auto scheduled_events = impl_->events.DrainQueued();
    for (const auto& event : scheduled_events) {
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
    snapshot.scheduler = Scheduler();
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
    replay.checkpoints.push_back({impl_->frame_index, Hashes()});
    replay.hashes = Hashes();
    return replay;
}

Astra::Core::VersionedDocument RuntimeWorld::Save() const {
    return {SnapshotSchema, 1, "runtime:/world", ToJson(CaptureSnapshot())};
}

SaveContainerV2 RuntimeWorld::SaveV2(bool compress_sections) const {
    SaveContainerV2 container;
    const auto build_info = Astra::Core::VersionedDocument{SnapshotSchema, 1, "runtime:/world", ToJson(CaptureSnapshot())};
    container.header = {
        {"engine_version", "0.2.0"},
        {"save_schema", SaveContainerV2Schema},
        {"created_frame", impl_->frame_index},
        {"project_version", 1},
        {"module_versions", nlohmann::json::object()},
        {"schema_versions", {{"runtime_snapshot", 1}, {"scheduler", 1}, {"replay", 1}}},
    };
    std::vector<std::pair<std::string, nlohmann::json>> section_payloads = {
        {"runtime_snapshot", build_info.payload},
        {"scheduler", ToJson(Scheduler())},
        {"replay", ToJson(CaptureReplay())},
        {"script_state", nlohmann::json::object()},
        {"media_state", nlohmann::json::object()},
        {"resource_overrides", nlohmann::json::object()},
        {"ai_committed_output", nlohmann::json::array()},
        {"module_extension_state", nlohmann::json::object()},
    };
    for (const auto& [name, payload] : section_payloads) {
        SaveSection section;
        section.name = name;
        section.schema = name == "runtime_snapshot" ? SnapshotSchema : "astra.runtime." + name + ".v1";
        section.payload = payload;
        const auto dumped = payload.dump();
        section.hash = StableHash(dumped);
        if (compress_sections) {
            auto compressed = CompressText(dumped);
            if (!compressed.empty()) {
                section.compressed = true;
                section.compressed_payload = std::move(compressed);
                section.payload = nlohmann::json::object();
            }
        }
        container.sections.push_back(std::move(section));
    }
    return container;
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
    impl_->tasks.clear();
    for (const auto& task : snapshot.Value().scheduler.tasks) {
        impl_->tasks[task.id.ToString()] = task;
    }
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

Astra::Core::Result<void> RuntimeWorld::Load(const SaveContainerV2& container, Astra::Core::DiagnosticSink& diagnostics) {
    if (container.schema != SaveContainerV2Schema) {
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "unsupported runtime save container schema");
    }
    const SaveSection* runtime_section = nullptr;
    for (const auto& section : container.sections) {
        if (section.name == "runtime_snapshot") {
            runtime_section = &section;
            break;
        }
    }
    if (runtime_section == nullptr) {
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "runtime snapshot section is missing");
    }
    nlohmann::json payload = runtime_section->payload;
    if (runtime_section->compressed) {
        auto decompressed = DecompressText(runtime_section->compressed_payload);
        if (!decompressed) {
            return Astra::Core::Result<void>::Failure(decompressed.Error(), decompressed.Message());
        }
        payload = nlohmann::json::parse(decompressed.Value(), nullptr, false);
        if (payload.is_discarded()) {
            return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "compressed runtime snapshot is invalid JSON");
        }
    }
    return Load({SnapshotSchema, 1, "runtime:/world", payload}, diagnostics);
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
        report.localized_mismatches.push_back({"state_hash", 0, 0, "", "", "", expected.state_hash, actual.state_hash});
    }
    if (expected.event_hash != actual.event_hash) {
        report.mismatches.push_back("event_hash");
        report.localized_mismatches.push_back({"event_hash", 0, 0, "", "", "", expected.event_hash, actual.event_hash});
    }
    if (expected.presentation_hash != actual.presentation_hash) {
        report.mismatches.push_back("presentation_hash");
        report.localized_mismatches.push_back({"presentation_hash", 0, 0, "", "", "", expected.presentation_hash, actual.presentation_hash});
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
