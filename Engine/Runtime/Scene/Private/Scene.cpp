#include <Astra/Scene/Scene.hpp>

#include <entt/entity/registry.hpp>

#include <algorithm>
#include <map>
#include <unordered_map>

namespace Astra::Scene {

namespace {

constexpr const char* TransformType = "astra.transform2d";
constexpr const char* TagType = "astra.tag";
constexpr const char* LifetimeType = "astra.lifetime";
constexpr const char* BlackboardType = "astra.blackboard";
constexpr const char* ControlPolicyType = "astra.control_policy";
constexpr const char* StateMachineType = "astra.state_machine";

struct ActorRecord {
    ActorId id;
    ActorTypeId type_id;
    std::string name;
    Astra::Core::u32 generation = 1;
    ActorLifecycleState lifecycle = ActorLifecycleState::Draft;
    std::map<std::string, ComponentData> components;
};

std::string Key(const ActorId& id) {
    return id.ToString();
}

void EmitInvalidHandle(Astra::Core::DiagnosticSink& diagnostics, const ActorHandle& handle) {
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = "ASTRA_SCENE_INVALID_ACTOR_HANDLE";
    diagnostic.category = "scene.actor";
    diagnostic.severity = Astra::Core::DiagnosticSeverity::Error;
    diagnostic.message = "Actor handle is missing, stale, or destroyed.";
    diagnostic.objects = {{"actor", handle.id.ToString()}};
    diagnostics.Emit(std::move(diagnostic));
}

std::string LifecycleToString(ActorLifecycleState state) {
    switch (state) {
    case ActorLifecycleState::Draft:
        return "draft";
    case ActorLifecycleState::Active:
        return "active";
    case ActorLifecycleState::Inactive:
        return "inactive";
    case ActorLifecycleState::Destroyed:
        return "destroyed";
    }
    return "draft";
}

ActorLifecycleState LifecycleFromString(std::string_view value) {
    if (value == "active") {
        return ActorLifecycleState::Active;
    }
    if (value == "inactive") {
        return ActorLifecycleState::Inactive;
    }
    if (value == "destroyed") {
        return ActorLifecycleState::Destroyed;
    }
    return ActorLifecycleState::Draft;
}

ComponentData BuiltinComponent(std::string type_id, nlohmann::json data) {
    auto id = Astra::Core::ParseStableId("component:/" + type_id);
    return {id ? id.Value() : ComponentId(Astra::Core::StableIdKind::Component, type_id), std::move(type_id), 1, std::move(data)};
}

} // namespace

class HeadlessLocalEcsPack final : public LocalEcsPack {
public:
    void SyncIn(const EcsPackInput& input) override {
        registry_.clear();
        actors_.clear();
        for (const auto& actor : input.actors) {
            auto entity = registry_.create();
            actors_.emplace(entity, actor);
        }
    }

    void Update(double fixed_delta_seconds) override {
        last_delta_seconds_ = fixed_delta_seconds;
    }

    EcsPackOutput SyncOut() const override {
        EcsPackOutput output;
        for (const auto& [entity, actor] : actors_) {
            (void)entity;
            output.actors.push_back(actor);
        }
        std::ranges::sort(output.actors, {}, [](const ActorSnapshot& actor) { return actor.id.ToString(); });
        output.emitted_events.push_back({{"type", "astra.ecs_pack.updated"}, {"fixed_delta_seconds", last_delta_seconds_}, {"actors", output.actors.size()}});
        return output;
    }

private:
    entt::registry registry_;
    std::unordered_map<entt::entity, ActorSnapshot> actors_;
    double last_delta_seconds_ = 0.0;
};

class ActorWorld::Impl {
public:
    entt::registry registry;
    std::unordered_map<std::string, entt::entity> entities_by_id;
    std::unordered_map<entt::entity, ActorRecord> records;
};

ActorWorld::ActorWorld() : impl_(std::make_unique<Impl>()) {}
ActorWorld::ActorWorld(ActorWorld&&) noexcept = default;
ActorWorld& ActorWorld::operator=(ActorWorld&&) noexcept = default;
ActorWorld::~ActorWorld() = default;

std::unique_ptr<LocalEcsPack> CreateHeadlessLocalEcsPack() {
    return std::make_unique<HeadlessLocalEcsPack>();
}

std::vector<ComponentDescriptor> FoundationComponentDescriptors() {
    return {
        {BuiltinComponent(TransformType, ToJson(Transform2DComponent{})).id, TransformType, 1, ToJson(Transform2DComponent{}), false},
        {BuiltinComponent(TagType, ToJson(TagComponent{})).id, TagType, 1, ToJson(TagComponent{}), false},
        {BuiltinComponent(LifetimeType, ToJson(LifetimeComponent{})).id, LifetimeType, 1, ToJson(LifetimeComponent{}), true},
        {BuiltinComponent(BlackboardType, ToJson(BlackboardComponent{})).id, BlackboardType, 1, ToJson(BlackboardComponent{}), false},
        {BuiltinComponent(ControlPolicyType, ToJson(ControlPolicyComponent{})).id, ControlPolicyType, 1, ToJson(ControlPolicyComponent{}), true},
        {BuiltinComponent(StateMachineType, ToJson(StateMachineComponent{})).id, StateMachineType, 1, ToJson(StateMachineComponent{}), true},
    };
}

Astra::Core::Result<ActorHandle> ActorWorld::Spawn(const ActorDescriptor& descriptor, Astra::Core::DiagnosticSink& diagnostics) {
    if (descriptor.id.Empty()) {
        Astra::Core::Diagnostic diagnostic;
        diagnostic.code = "ASTRA_SCENE_ACTOR_ID_REQUIRED";
        diagnostic.category = "scene.actor";
        diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
        diagnostic.message = "Actor spawn requires a stable ActorId.";
        diagnostics.Emit(std::move(diagnostic));
        return Astra::Core::Result<ActorHandle>::Failure(Astra::Core::ErrorCode::InvalidArgument, "actor id is required");
    }
    const auto key = Key(descriptor.id);
    if (impl_->entities_by_id.contains(key)) {
        Astra::Core::Diagnostic diagnostic;
        diagnostic.code = "ASTRA_SCENE_ACTOR_DUPLICATE";
        diagnostic.category = "scene.actor";
        diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
        diagnostic.message = "ActorId already exists in world.";
        diagnostic.objects = {{"actor", descriptor.id.ToString()}};
        diagnostics.Emit(std::move(diagnostic));
        return Astra::Core::Result<ActorHandle>::Failure(Astra::Core::ErrorCode::InvalidArgument, "duplicate actor id");
    }

    auto entity = impl_->registry.create();
    ActorRecord record;
    record.id = descriptor.id;
    record.type_id = descriptor.type_id;
    record.name = descriptor.name;
    record.lifecycle = ActorLifecycleState::Draft;
    for (const auto& component : FoundationComponentDescriptors()) {
        record.components[component.type_id] = {component.id, component.type_id, component.version, component.default_data};
    }
    for (const auto& component : descriptor.components) {
        record.components[component.type_id] = component;
    }

    const auto generation = record.generation;
    impl_->entities_by_id[key] = entity;
    impl_->records.emplace(entity, std::move(record));
    return Astra::Core::Result<ActorHandle>::Success({descriptor.id, generation});
}

bool ActorWorld::IsHandleAlive(const ActorHandle& handle) const {
    auto entity_it = impl_->entities_by_id.find(Key(handle.id));
    if (entity_it == impl_->entities_by_id.end()) {
        return false;
    }
    auto record_it = impl_->records.find(entity_it->second);
    return record_it != impl_->records.end() && record_it->second.generation == handle.generation && record_it->second.lifecycle != ActorLifecycleState::Destroyed;
}

Astra::Core::Result<void> ActorWorld::Activate(const ActorHandle& handle, Astra::Core::DiagnosticSink& diagnostics) {
    if (!IsHandleAlive(handle)) {
        EmitInvalidHandle(diagnostics, handle);
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::NotFound, "invalid actor handle");
    }
    impl_->records.at(impl_->entities_by_id.at(Key(handle.id))).lifecycle = ActorLifecycleState::Active;
    return Astra::Core::Result<void>::Success();
}

Astra::Core::Result<void> ActorWorld::Deactivate(const ActorHandle& handle, Astra::Core::DiagnosticSink& diagnostics) {
    if (!IsHandleAlive(handle)) {
        EmitInvalidHandle(diagnostics, handle);
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::NotFound, "invalid actor handle");
    }
    impl_->records.at(impl_->entities_by_id.at(Key(handle.id))).lifecycle = ActorLifecycleState::Inactive;
    return Astra::Core::Result<void>::Success();
}

Astra::Core::Result<void> ActorWorld::Destroy(const ActorHandle& handle, Astra::Core::DiagnosticSink& diagnostics) {
    if (!IsHandleAlive(handle)) {
        EmitInvalidHandle(diagnostics, handle);
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::NotFound, "invalid actor handle");
    }
    const auto key = Key(handle.id);
    const auto entity = impl_->entities_by_id.at(key);
    impl_->records.at(entity).lifecycle = ActorLifecycleState::Destroyed;
    impl_->records.at(entity).generation += 1;
    impl_->records.erase(entity);
    impl_->entities_by_id.erase(key);
    impl_->registry.destroy(entity);
    return Astra::Core::Result<void>::Success();
}

Astra::Core::Result<void> ActorWorld::AddOrReplaceComponent(const ActorHandle& handle, ComponentData component, Astra::Core::DiagnosticSink& diagnostics) {
    if (!IsHandleAlive(handle)) {
        EmitInvalidHandle(diagnostics, handle);
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::NotFound, "invalid actor handle");
    }
    if (component.type_id.empty()) {
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidArgument, "component type id is required");
    }
    impl_->records.at(impl_->entities_by_id.at(Key(handle.id))).components[component.type_id] = std::move(component);
    return Astra::Core::Result<void>::Success();
}

std::optional<ComponentData> ActorWorld::FindComponent(const ActorHandle& handle, std::string_view type_id) const {
    if (!IsHandleAlive(handle)) {
        return std::nullopt;
    }
    const auto& components = impl_->records.at(impl_->entities_by_id.at(Key(handle.id))).components;
    auto it = components.find(std::string(type_id));
    if (it == components.end()) {
        return std::nullopt;
    }
    return it->second;
}

std::optional<ActorHandle> ActorWorld::ResolveActor(const ActorId& id, Astra::Core::DiagnosticSink& diagnostics) const {
    const auto entity_it = impl_->entities_by_id.find(Key(id));
    if (entity_it == impl_->entities_by_id.end()) {
        Astra::Core::Diagnostic diagnostic;
        diagnostic.code = "ASTRA_SCENE_ACTOR_REFERENCE_MISSING";
        diagnostic.category = "scene.reference";
        diagnostic.severity = Astra::Core::DiagnosticSeverity::Warning;
        diagnostic.message = "Actor reference could not be resolved.";
        diagnostic.objects = {{"actor", id.ToString()}};
        diagnostics.Emit(std::move(diagnostic));
        return std::nullopt;
    }
    const auto& record = impl_->records.at(entity_it->second);
    if (record.lifecycle == ActorLifecycleState::Destroyed) {
        EmitInvalidHandle(diagnostics, {id, record.generation});
        return std::nullopt;
    }
    return ActorHandle{id, record.generation};
}

std::optional<ActorSnapshot> ActorWorld::SnapshotActor(const ActorHandle& handle) const {
    if (!IsHandleAlive(handle)) {
        return std::nullopt;
    }
    const auto& record = impl_->records.at(impl_->entities_by_id.at(Key(handle.id)));
    ActorSnapshot snapshot;
    snapshot.id = record.id;
    snapshot.type_id = record.type_id;
    snapshot.name = record.name;
    snapshot.generation = record.generation;
    snapshot.lifecycle = record.lifecycle;
    for (const auto& [type, component] : record.components) {
        (void)type;
        snapshot.components.push_back(component);
    }
    return snapshot;
}

WorldSnapshot ActorWorld::Snapshot() const {
    WorldSnapshot snapshot;
    for (const auto& [entity, record] : impl_->records) {
        (void)entity;
        ActorSnapshot actor;
        actor.id = record.id;
        actor.type_id = record.type_id;
        actor.name = record.name;
        actor.generation = record.generation;
        actor.lifecycle = record.lifecycle;
        for (const auto& [type, component] : record.components) {
            (void)type;
            actor.components.push_back(component);
        }
        snapshot.actors.push_back(std::move(actor));
    }
    std::ranges::sort(snapshot.actors, {}, [](const ActorSnapshot& actor) { return actor.id.ToString(); });
    return snapshot;
}

Astra::Core::Result<void> ActorWorld::Restore(const WorldSnapshot& snapshot, Astra::Core::DiagnosticSink&) {
    impl_->registry.clear();
    impl_->entities_by_id.clear();
    impl_->records.clear();
    for (const auto& actor : snapshot.actors) {
        const auto entity = impl_->registry.create();
        ActorRecord record;
        record.id = actor.id;
        record.type_id = actor.type_id;
        record.name = actor.name;
        record.generation = actor.generation;
        record.lifecycle = actor.lifecycle;
        for (const auto& component : actor.components) {
            record.components[component.type_id] = component;
        }
        impl_->entities_by_id[Key(actor.id)] = entity;
        impl_->records.emplace(entity, std::move(record));
    }
    return Astra::Core::Result<void>::Success();
}

std::size_t ActorWorld::ActorCount() const {
    return impl_->records.size();
}

nlohmann::json ToJson(const Transform2DComponent& component) {
    return {{"x", component.x}, {"y", component.y}, {"rotation_degrees", component.rotation_degrees}, {"scale_x", component.scale_x}, {"scale_y", component.scale_y}};
}

nlohmann::json ToJson(const TagComponent& component) {
    return {{"tags", component.tags}};
}

nlohmann::json ToJson(const LifetimeComponent& component) {
    return {{"active", component.active}, {"pending_destroy", component.pending_destroy}};
}

nlohmann::json ToJson(const BlackboardComponent& component) {
    return component.values;
}

nlohmann::json ToJson(const ControlPolicyComponent& component) {
    return {{"owner", component.owner}, {"locked_channels", component.locked_channels}, {"queued_events", component.queued_events}, {"ai_allowed", component.ai_allowed}};
}

nlohmann::json ToJson(const StateMachineComponent& component) {
    return {{"state_machine_id", component.state_machine_id.ToString()}, {"current_state", component.current_state}, {"delayed_events", component.delayed_events}};
}

nlohmann::json ToJson(const ComponentDescriptor& descriptor) {
    return {{"id", descriptor.id.ToString()}, {"type_id", descriptor.type_id}, {"version", descriptor.version}, {"default_data", descriptor.default_data}, {"runtime_managed", descriptor.runtime_managed}};
}

nlohmann::json ToJson(const ComponentData& component) {
    return {{"id", component.id.ToString()}, {"type_id", component.type_id}, {"version", component.version}, {"data", component.data}};
}

nlohmann::json ToJson(const ActorSnapshot& actor) {
    nlohmann::json components = nlohmann::json::array();
    for (const auto& component : actor.components) {
        components.push_back(ToJson(component));
    }
    return {
        {"id", actor.id.ToString()},
        {"type_id", actor.type_id.ToString()},
        {"name", actor.name},
        {"generation", actor.generation},
        {"lifecycle", LifecycleToString(actor.lifecycle)},
        {"components", components},
    };
}

nlohmann::json ToJson(const WorldSnapshot& world) {
    nlohmann::json actors = nlohmann::json::array();
    for (const auto& actor : world.actors) {
        actors.push_back(ToJson(actor));
    }
    return {{"schema", world.schema}, {"version", world.version}, {"actors", actors}};
}

Astra::Core::Result<WorldSnapshot> WorldSnapshotFromJson(const nlohmann::json& json) {
    if (!json.contains("actors")) {
        return Astra::Core::Result<WorldSnapshot>::Failure(Astra::Core::ErrorCode::InvalidFormat, "world snapshot requires actors");
    }
    WorldSnapshot world;
    world.schema = json.value("schema", "astra.scene.world.v1");
    world.version = json.value("version", 1u);
    for (const auto& actor_json : json.at("actors")) {
        auto actor_id = Astra::Core::ParseStableId(actor_json.at("id").get<std::string>());
        auto type_id = Astra::Core::ParseStableId(actor_json.at("type_id").get<std::string>());
        if (!actor_id || !type_id) {
            return Astra::Core::Result<WorldSnapshot>::Failure(Astra::Core::ErrorCode::InvalidFormat, "actor snapshot has invalid stable id");
        }
        ActorSnapshot actor;
        actor.id = actor_id.Value();
        actor.type_id = type_id.Value();
        actor.name = actor_json.value("name", "");
        actor.generation = actor_json.value("generation", 1u);
        actor.lifecycle = LifecycleFromString(actor_json.value("lifecycle", "draft"));
        for (const auto& component_json : actor_json.value("components", nlohmann::json::array())) {
            auto component_id = Astra::Core::ParseStableId(component_json.at("id").get<std::string>());
            if (!component_id) {
                return Astra::Core::Result<WorldSnapshot>::Failure(Astra::Core::ErrorCode::InvalidFormat, "component snapshot has invalid stable id");
            }
            actor.components.push_back({component_id.Value(), component_json.at("type_id").get<std::string>(), component_json.value("version", 1u), component_json.value("data", nlohmann::json::object())});
        }
        world.actors.push_back(std::move(actor));
    }
    return Astra::Core::Result<WorldSnapshot>::Success(std::move(world));
}

} // namespace Astra::Scene
