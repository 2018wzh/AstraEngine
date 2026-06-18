#pragma once

#include <Astra/Core/Diagnostics.hpp>
#include <Astra/Core/Serialization.hpp>
#include <Astra/Core/StableId.hpp>
#include <Astra/Core/Types.hpp>
#include <Astra/Runtime/Export.hpp>
#include <Astra/Scene/Scene.hpp>
#include <nlohmann/json.hpp>

#include <deque>
#include <map>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

namespace Astra::Runtime {

constexpr const char* SnapshotSchema = "astra.runtime.snapshot.v1";
constexpr const char* ReplaySchema = "astra.runtime.replay.v1";
constexpr const char* SaveContainerSchema = "astra.runtime.save_container.v1";
constexpr const char* SaveContainerV2Schema = "astra.runtime.save_container.v2";
constexpr const char* ReplayComparisonSchema = "astra.runtime.replay_comparison.v1";

enum class RuntimeEventMode {
    Immediate,
    Queued,
    Deferred
};

struct RuntimeEventEndpoint {
    std::string kind;
    std::string id;
};

struct RuntimeEventTrace {
    std::string script_location;
    std::string audit_ref;
};

struct RuntimeEvent {
    Astra::Core::StableId event_id;
    Astra::Core::EventTypeId type;
    std::string category = "runtime";
    Astra::Core::u32 priority = 0;
    Astra::Core::u64 sequence = 0;
    Astra::Core::u64 frame_index = 0;
    RuntimeEventEndpoint source;
    RuntimeEventEndpoint target;
    std::string payload_schema;
    nlohmann::json payload = nlohmann::json::object();
    RuntimeEventTrace trace;
};

struct RuntimeEventSubscription {
    Astra::Core::StableId id;
    Astra::Core::EventTypeId event_type;
    std::string owner;
    bool active = true;
};

struct StateTransition {
    std::string from_state;
    Astra::Core::EventTypeId event_type;
    std::string to_state;
    Astra::Core::u32 priority = 0;
    nlohmann::json guard = nlohmann::json::object();
    std::vector<nlohmann::json> actions;
};

struct StateMachineDefinition {
    Astra::Core::StableId id;
    std::string initial_state = "idle";
    std::vector<StateTransition> transitions;
};

struct DirectorState {
    std::string phase = "runtime";
    bool timeline_locked = false;
    bool choice_locked = false;
    bool ai_permission_window = false;
    std::string player_input_window = "closed";
    nlohmann::json arbitration_log = nlohmann::json::array();
};

enum class ControlDecision {
    Allow,
    Queue,
    Reject
};

struct ControlPolicyRequest {
    Astra::Scene::ActorId actor_id;
    std::string channel;
    std::string requester;
    Astra::Core::u32 priority = 0;
    std::string interrupt_policy = "queue";
};

struct ControlPolicyResult {
    ControlDecision decision = ControlDecision::Allow;
    std::string reason;
};

struct RuntimeHashes {
    std::string state_hash;
    std::string event_hash;
    std::string presentation_hash;
};

enum class RuntimeTaskState {
    Pending,
    Running,
    Waiting,
    Cancelled,
    Completed
};

enum class RuntimeWaitKind {
    None,
    Event,
    Time,
    FixedSteps,
    Asset,
    AssetReady,
    Script,
    Debugger,
    ProviderSignal
};

struct RuntimeWaitCondition {
    RuntimeWaitKind kind = RuntimeWaitKind::None;
    Astra::Core::EventTypeId event_type;
    std::string asset;
    Astra::Core::u64 wake_frame = 0;
    std::string script_state;
};

struct RuntimeTask {
    Astra::Core::StableId id;
    RuntimeTaskState state = RuntimeTaskState::Pending;
    RuntimeEventEndpoint owner;
    RuntimeWaitCondition wait;
    nlohmann::json continuation = nlohmann::json::object();
    RuntimeEvent emit_on_wake;
    std::string cancellation_policy = "cancel_on_owner_destroy";
};

struct SchedulerSnapshot {
    std::string schema = "astra.runtime.scheduler.v1";
    std::vector<RuntimeTask> tasks;
};

struct ReplayCheckpoint {
    Astra::Core::u64 frame_index = 0;
    RuntimeHashes hashes;
};

struct ReplayMismatch {
    std::string kind;
    Astra::Core::u64 frame_index = 0;
    Astra::Core::u64 event_sequence = 0;
    std::string actor;
    std::string component;
    std::string script_location;
    std::string provider_id;
    std::string asset_or_pass;
    std::string expected;
    std::string actual;
};

struct RuntimeReplay {
    std::string schema = ReplaySchema;
    Astra::Core::u32 version = 1;
    Astra::Core::u64 random_seed = 0;
    nlohmann::json inputs = nlohmann::json::array();
    nlohmann::json script_decisions = nlohmann::json::array();
    nlohmann::json choice_selections = nlohmann::json::array();
    nlohmann::json committed_ai_output = nlohmann::json::array();
    nlohmann::json events = nlohmann::json::array();
    std::vector<ReplayCheckpoint> checkpoints;
    RuntimeHashes hashes;
};

struct ReplayComparisonReport {
    std::string schema = ReplayComparisonSchema;
    bool passed = false;
    RuntimeHashes expected;
    RuntimeHashes actual;
    std::vector<std::string> mismatches;
    std::vector<ReplayMismatch> localized_mismatches;
};

struct RuntimeSnapshot {
    std::string schema = SnapshotSchema;
    Astra::Core::u32 version = 1;
    Astra::Core::u64 frame_index = 0;
    Astra::Core::u64 fixed_step_index = 0;
    Astra::Core::u64 next_event_sequence = 1;
    Astra::Core::u64 random_seed = 0;
    Astra::Scene::WorldSnapshot world;
    DirectorState director;
    SchedulerSnapshot scheduler;
    nlohmann::json replay_events = nlohmann::json::array();
    RuntimeHashes hashes;
};

struct SaveContainer {
    std::string schema = SaveContainerSchema;
    Astra::Core::u32 version = 1;
    std::string engine_version;
    std::string package_hash;
    Astra::Core::u64 created_frame = 0;
    RuntimeSnapshot runtime_snapshot;
    nlohmann::json script_snapshot = nlohmann::json::object();
    nlohmann::json media_state = nlohmann::json::object();
};

struct SaveSection {
    std::string name;
    std::string schema;
    Astra::Core::u32 version = 1;
    std::string hash;
    bool compressed = false;
    nlohmann::json payload = nlohmann::json::object();
    std::vector<Astra::Core::u8> compressed_payload;
};

struct SaveContainerV2 {
    std::string schema = SaveContainerV2Schema;
    Astra::Core::u32 version = 2;
    nlohmann::json header = nlohmann::json::object();
    std::vector<SaveSection> sections;
};

class ASTRA_RUNTIME_API RuntimeEventBus {
public:
    void Emit(RuntimeEvent event, RuntimeEventMode mode);
    [[nodiscard]] std::vector<RuntimeEvent> DrainQueued();
    void AdvanceDeferred();
    [[nodiscard]] std::size_t QueuedCount() const;
    [[nodiscard]] const std::vector<RuntimeEvent>& Trace() const;
    void RestoreTrace(std::vector<RuntimeEvent> trace);
    void Clear();

private:
    std::deque<RuntimeEvent> queued_;
    std::deque<RuntimeEvent> deferred_;
    std::vector<RuntimeEvent> trace_;
};

class ASTRA_RUNTIME_API RuntimeWorld {
public:
    explicit RuntimeWorld(Astra::Core::u64 random_seed = 0);
    RuntimeWorld(RuntimeWorld&&) noexcept;
    RuntimeWorld& operator=(RuntimeWorld&&) noexcept;
    ~RuntimeWorld();

    RuntimeWorld(const RuntimeWorld&) = delete;
    RuntimeWorld& operator=(const RuntimeWorld&) = delete;

    [[nodiscard]] Astra::Scene::ActorWorld& Scene();
    [[nodiscard]] const Astra::Scene::ActorWorld& Scene() const;
    [[nodiscard]] RuntimeEventBus& Events();
    [[nodiscard]] const RuntimeEventBus& Events() const;
    [[nodiscard]] const DirectorState& Director() const;
    void SetDirector(DirectorState state);

    void RegisterStateMachine(StateMachineDefinition definition);
    [[nodiscard]] RuntimeEventSubscription Subscribe(Astra::Core::EventTypeId event_type, std::string owner);
    [[nodiscard]] Astra::Core::Result<void> Unsubscribe(const Astra::Core::StableId& subscription, Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] Astra::Core::Result<void> ScheduleTask(RuntimeTask task, Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] Astra::Core::Result<void> CancelTask(const Astra::Core::StableId& task_id, std::string reason, Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] SchedulerSnapshot Scheduler() const;
    [[nodiscard]] ControlPolicyResult EvaluateControlPolicy(const ControlPolicyRequest& request, Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] Astra::Core::Result<void> Emit(RuntimeEvent event, RuntimeEventMode mode, Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] Astra::Core::Result<void> Tick(Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] RuntimeSnapshot CaptureSnapshot() const;
    [[nodiscard]] RuntimeReplay CaptureReplay() const;
    [[nodiscard]] Astra::Core::VersionedDocument Save() const;
    [[nodiscard]] SaveContainerV2 SaveV2(bool compress_sections) const;
    [[nodiscard]] Astra::Core::Result<void> Load(const Astra::Core::VersionedDocument& document, Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] Astra::Core::Result<void> Load(const SaveContainerV2& container, Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] RuntimeHashes Hashes() const;

private:
    class Impl;
    std::unique_ptr<Impl> impl_;
};

[[nodiscard]] ASTRA_RUNTIME_API nlohmann::json ToJson(const RuntimeEventEndpoint& endpoint);
[[nodiscard]] ASTRA_RUNTIME_API nlohmann::json ToJson(const RuntimeEventTrace& trace);
[[nodiscard]] ASTRA_RUNTIME_API nlohmann::json ToJson(const RuntimeEvent& event);
[[nodiscard]] ASTRA_RUNTIME_API nlohmann::json ToJson(const RuntimeEventSubscription& subscription);
[[nodiscard]] ASTRA_RUNTIME_API nlohmann::json ToJson(const DirectorState& state);
[[nodiscard]] ASTRA_RUNTIME_API nlohmann::json ToJson(const ControlPolicyResult& result);
[[nodiscard]] ASTRA_RUNTIME_API nlohmann::json ToJson(const RuntimeWaitCondition& wait);
[[nodiscard]] ASTRA_RUNTIME_API nlohmann::json ToJson(const RuntimeTask& task);
[[nodiscard]] ASTRA_RUNTIME_API nlohmann::json ToJson(const SchedulerSnapshot& scheduler);
[[nodiscard]] ASTRA_RUNTIME_API nlohmann::json ToJson(const RuntimeHashes& hashes);
[[nodiscard]] ASTRA_RUNTIME_API nlohmann::json ToJson(const RuntimeReplay& replay);
[[nodiscard]] ASTRA_RUNTIME_API nlohmann::json ToJson(const SaveContainer& container);
[[nodiscard]] ASTRA_RUNTIME_API nlohmann::json ToJson(const SaveSection& section);
[[nodiscard]] ASTRA_RUNTIME_API nlohmann::json ToJson(const SaveContainerV2& container);
[[nodiscard]] ASTRA_RUNTIME_API nlohmann::json ToJson(const ReplayComparisonReport& report);
[[nodiscard]] ASTRA_RUNTIME_API nlohmann::json ToJson(const RuntimeSnapshot& snapshot);
[[nodiscard]] ASTRA_RUNTIME_API Astra::Core::Result<RuntimeEvent> RuntimeEventFromJson(const nlohmann::json& json);
[[nodiscard]] ASTRA_RUNTIME_API Astra::Core::Result<RuntimeSnapshot> RuntimeSnapshotFromJson(const nlohmann::json& json);
[[nodiscard]] ASTRA_RUNTIME_API Astra::Core::Result<SaveContainerV2> SaveContainerV2FromJson(const nlohmann::json& json);
[[nodiscard]] ASTRA_RUNTIME_API ReplayComparisonReport CompareReplayHashes(const RuntimeHashes& expected, const RuntimeHashes& actual);

} // namespace Astra::Runtime
