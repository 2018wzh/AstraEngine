#include <Astra/Runtime/Runtime.hpp>

#include <Astra/Core/Logging.hpp>

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
    Astra::Core::LogEvent log;
    log.channel = "runtime.event";
    log.component = event.type.ToString();
    log.level = mode == RuntimeEventMode::Immediate ? Astra::Core::LogLevel::Debug : Astra::Core::LogLevel::Trace;
    log.message = "runtime event emitted";
    log.frame_index = event.frame_index;
    log.objects = {{"event", event.event_id.ToString()}};
    log.fields = {{"type", event.type.ToString()},
                  {"mode", EventModeToString(mode)},
                  {"sequence", std::to_string(event.sequence)},
                  {"source", event.source.id},
                  {"target", event.target.id}};
    Astra::Core::DefaultLogger().Write(std::move(log));
    const auto copied = event;
    impl_->events.Emit(std::move(event), mode);
    if (mode == RuntimeEventMode::Immediate) {
        impl_->ApplyStateMachines(copied);
    }
    return Astra::Core::Result<void>::Success();
}

Astra::Core::Result<void> RuntimeWorld::Tick(Astra::Core::DiagnosticSink&) {
    Astra::Core::DefaultLogger().Log(
        "runtime.tick",
        "runtime_world",
        Astra::Core::LogLevel::Trace,
        "runtime tick started",
        {{"frame", std::to_string(impl_->frame_index)}, {"queued", std::to_string(impl_->events.QueuedCount())}});
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
    Astra::Core::DefaultLogger().Log(
        "runtime.tick",
        "runtime_world",
        Astra::Core::LogLevel::Trace,
        "runtime tick finished",
        {{"frame", std::to_string(impl_->frame_index)},
         {"events", std::to_string(events.size() + scheduled_events.size())},
         {"tasks", std::to_string(impl_->tasks.size())}});
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
    Astra::Core::DefaultLogger().Log(
        "runtime.save",
        "runtime_world",
        Astra::Core::LogLevel::Debug,
        "runtime save captured",
        {{"frame", std::to_string(impl_->frame_index)}});
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
        {"schema_versions", {{"runtime_snapshot", 1}, {"scheduler", 1}, {"replay", 1}, {"media_logical_state", 1}}},
    };
    std::vector<std::pair<std::string, nlohmann::json>> section_payloads = {
        {"section:/runtime/world", build_info.payload},
        {"section:/runtime/scheduler", ToJson(Scheduler())},
        {"section:/runtime/replay", ToJson(CaptureReplay())},
        {"section:/script/runtime", nlohmann::json::object()},
        {"section:/media/logical_state", {{"schema", "astra.media.logical_state.v1"}, {"provider_hashes", nlohmann::json::object()}, {"timeline_states", nlohmann::json::array()}, {"audio_state", nlohmann::json::object()}, {"filter_state", nlohmann::json::object()}}},
        {"section:/media/resource_overrides", nlohmann::json::object()},
        {"section:/ai/committed_output", nlohmann::json::array()},
        {"section:/modules/extension_state", nlohmann::json::object()},
    };
    for (const auto& [name, payload] : section_payloads) {
        SaveSection section;
        section.name = name;
        section.schema = name == "section:/runtime/world" ? SnapshotSchema : "astra.runtime." + name.substr(std::string("section:/").size()) + ".v1";
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
    Astra::Core::DefaultLogger().Log(
        "runtime.save",
        "runtime_world",
        Astra::Core::LogLevel::Debug,
        "runtime load started",
        {{"schema", document.schema}, {"object", document.object_id}});
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
    Astra::Core::DefaultLogger().Log(
        "runtime.save",
        "runtime_world",
        Astra::Core::LogLevel::Debug,
        "runtime load finished",
        {{"frame", std::to_string(impl_->frame_index)}, {"tasks", std::to_string(impl_->tasks.size())}});
    return Astra::Core::Result<void>::Success();
}

Astra::Core::Result<void> RuntimeWorld::Load(const SaveContainerV2& container, Astra::Core::DiagnosticSink& diagnostics) {
    if (container.schema != SaveContainerV2Schema) {
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "unsupported runtime save container schema");
    }
    const SaveSection* runtime_section = nullptr;
    for (const auto& section : container.sections) {
        if (section.name == "runtime_snapshot" || section.name == "section:/runtime/world") {
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

} // namespace Astra::Runtime
