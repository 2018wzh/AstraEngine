#pragma once

#include <Astra/Core/Diagnostics.hpp>
#include <Astra/Core/Serialization.hpp>
#include <Astra/Core/StableId.hpp>
#include <Astra/Core/Types.hpp>
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
    Astra::Core::u64 sequence = 0;
    Astra::Core::u64 frame_index = 0;
    RuntimeEventEndpoint source;
    RuntimeEventEndpoint target;
    std::string payload_schema;
    nlohmann::json payload = nlohmann::json::object();
    RuntimeEventTrace trace;
};

struct StateTransition {
    std::string from_state;
    Astra::Core::EventTypeId event_type;
    std::string to_state;
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

struct RuntimeReplay {
    std::string schema = ReplaySchema;
    Astra::Core::u32 version = 1;
    Astra::Core::u64 random_seed = 0;
    nlohmann::json events = nlohmann::json::array();
    RuntimeHashes hashes;
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
    nlohmann::json replay_events = nlohmann::json::array();
    RuntimeHashes hashes;
};

class RuntimeEventBus {
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

class RuntimeWorld {
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
    [[nodiscard]] ControlPolicyResult EvaluateControlPolicy(const ControlPolicyRequest& request, Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] Astra::Core::Result<void> Emit(RuntimeEvent event, RuntimeEventMode mode, Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] Astra::Core::Result<void> Tick(Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] RuntimeSnapshot CaptureSnapshot() const;
    [[nodiscard]] RuntimeReplay CaptureReplay() const;
    [[nodiscard]] Astra::Core::VersionedDocument Save() const;
    [[nodiscard]] Astra::Core::Result<void> Load(const Astra::Core::VersionedDocument& document, Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] RuntimeHashes Hashes() const;

private:
    class Impl;
    std::unique_ptr<Impl> impl_;
};

[[nodiscard]] nlohmann::json ToJson(const RuntimeEventEndpoint& endpoint);
[[nodiscard]] nlohmann::json ToJson(const RuntimeEventTrace& trace);
[[nodiscard]] nlohmann::json ToJson(const RuntimeEvent& event);
[[nodiscard]] nlohmann::json ToJson(const DirectorState& state);
[[nodiscard]] nlohmann::json ToJson(const ControlPolicyResult& result);
[[nodiscard]] nlohmann::json ToJson(const RuntimeHashes& hashes);
[[nodiscard]] nlohmann::json ToJson(const RuntimeReplay& replay);
[[nodiscard]] nlohmann::json ToJson(const RuntimeSnapshot& snapshot);
[[nodiscard]] Astra::Core::Result<RuntimeEvent> RuntimeEventFromJson(const nlohmann::json& json);
[[nodiscard]] Astra::Core::Result<RuntimeSnapshot> RuntimeSnapshotFromJson(const nlohmann::json& json);

} // namespace Astra::Runtime
