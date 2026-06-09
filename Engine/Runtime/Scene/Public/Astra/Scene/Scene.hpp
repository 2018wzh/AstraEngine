#pragma once

#include <Astra/Core/Diagnostics.hpp>
#include <Astra/Core/StableId.hpp>
#include <Astra/Core/Types.hpp>
#include <nlohmann/json.hpp>

#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace Astra::Scene {

using ActorId = Astra::Core::ActorId;
using ActorTypeId = Astra::Core::TypeId;
using ComponentId = Astra::Core::ComponentId;

enum class ActorLifecycleState {
    Draft,
    Active,
    Inactive,
    Destroyed
};

struct ActorHandle {
    ActorId id;
    Astra::Core::u32 generation = 0;

    [[nodiscard]] bool Empty() const { return id.Empty(); }
};

struct Transform2DComponent {
    double x = 0.0;
    double y = 0.0;
    double rotation_degrees = 0.0;
    double scale_x = 1.0;
    double scale_y = 1.0;
};

struct TagComponent {
    std::vector<std::string> tags;
};

struct LifetimeComponent {
    bool active = true;
    bool pending_destroy = false;
};

struct BlackboardComponent {
    nlohmann::json values = nlohmann::json::object();
};

struct ControlPolicyComponent {
    std::string owner = "system";
    std::vector<std::string> locked_channels;
    nlohmann::json queued_events = nlohmann::json::array();
    bool ai_allowed = false;
};

struct StateMachineComponent {
    Astra::Core::StableId state_machine_id;
    std::string current_state = "idle";
    nlohmann::json delayed_events = nlohmann::json::array();
};

struct ComponentData {
    ComponentId id;
    std::string type_id;
    Astra::Core::u32 version = 1;
    nlohmann::json data = nlohmann::json::object();
};

struct ComponentDescriptor {
    ComponentId id;
    std::string type_id;
    Astra::Core::u32 version = 1;
    nlohmann::json default_data = nlohmann::json::object();
    bool runtime_managed = false;
};

struct ActorDescriptor {
    ActorId id;
    ActorTypeId type_id;
    std::string name;
    std::vector<ComponentData> components;
};

struct ActorSnapshot {
    ActorId id;
    ActorTypeId type_id;
    std::string name;
    Astra::Core::u32 generation = 0;
    ActorLifecycleState lifecycle = ActorLifecycleState::Draft;
    std::vector<ComponentData> components;
};

struct WorldSnapshot {
    std::string schema = "astra.scene.world.v1";
    Astra::Core::u32 version = 1;
    std::vector<ActorSnapshot> actors;
};

struct EcsPackInput {
    std::vector<ActorSnapshot> actors;
};

struct EcsPackOutput {
    std::vector<ActorSnapshot> actors;
    std::vector<nlohmann::json> emitted_events;
};

class LocalEcsPack {
public:
    virtual ~LocalEcsPack() = default;
    virtual void SyncIn(const EcsPackInput& input) = 0;
    virtual void Update(double fixed_delta_seconds) = 0;
    [[nodiscard]] virtual EcsPackOutput SyncOut() const = 0;
};

[[nodiscard]] std::unique_ptr<LocalEcsPack> CreateHeadlessLocalEcsPack();
[[nodiscard]] std::vector<ComponentDescriptor> FoundationComponentDescriptors();

class ActorWorld {
public:
    ActorWorld();
    ActorWorld(ActorWorld&&) noexcept;
    ActorWorld& operator=(ActorWorld&&) noexcept;
    ~ActorWorld();

    ActorWorld(const ActorWorld&) = delete;
    ActorWorld& operator=(const ActorWorld&) = delete;

    [[nodiscard]] Astra::Core::Result<ActorHandle> Spawn(const ActorDescriptor& descriptor, Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] Astra::Core::Result<void> Activate(const ActorHandle& handle, Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] Astra::Core::Result<void> Deactivate(const ActorHandle& handle, Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] Astra::Core::Result<void> Destroy(const ActorHandle& handle, Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] Astra::Core::Result<void> AddOrReplaceComponent(const ActorHandle& handle, ComponentData component, Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] std::optional<ComponentData> FindComponent(const ActorHandle& handle, std::string_view type_id) const;
    [[nodiscard]] std::optional<ActorHandle> ResolveActor(const ActorId& id, Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] bool IsHandleAlive(const ActorHandle& handle) const;
    [[nodiscard]] std::optional<ActorSnapshot> SnapshotActor(const ActorHandle& handle) const;
    [[nodiscard]] WorldSnapshot Snapshot() const;
    [[nodiscard]] Astra::Core::Result<void> Restore(const WorldSnapshot& snapshot, Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] std::size_t ActorCount() const;

private:
    class Impl;
    std::unique_ptr<Impl> impl_;
};

[[nodiscard]] nlohmann::json ToJson(const Transform2DComponent& component);
[[nodiscard]] nlohmann::json ToJson(const TagComponent& component);
[[nodiscard]] nlohmann::json ToJson(const LifetimeComponent& component);
[[nodiscard]] nlohmann::json ToJson(const BlackboardComponent& component);
[[nodiscard]] nlohmann::json ToJson(const ControlPolicyComponent& component);
[[nodiscard]] nlohmann::json ToJson(const StateMachineComponent& component);
[[nodiscard]] nlohmann::json ToJson(const ComponentDescriptor& descriptor);
[[nodiscard]] nlohmann::json ToJson(const ComponentData& component);
[[nodiscard]] nlohmann::json ToJson(const ActorSnapshot& actor);
[[nodiscard]] nlohmann::json ToJson(const WorldSnapshot& world);
[[nodiscard]] Astra::Core::Result<WorldSnapshot> WorldSnapshotFromJson(const nlohmann::json& json);

} // namespace Astra::Scene
