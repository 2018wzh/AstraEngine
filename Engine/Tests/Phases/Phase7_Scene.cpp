TEST_CASE("Scene world spawns actors snapshots components and rejects stale handles") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Scene::ActorWorld world;

    auto actor_id = Astra::Core::ParseStableId("actor:/characters/alice");
    auto actor_type = Astra::Core::ParseStableId("type:/astra.vn.character");
    auto component_id = Astra::Core::ParseStableId("component:/astra.dialogue");
    REQUIRE(actor_id);
    REQUIRE(actor_type);
    REQUIRE(component_id);

    Astra::Scene::ActorDescriptor descriptor;
    descriptor.id = actor_id.Value();
    descriptor.type_id = actor_type.Value();
    descriptor.name = "Alice";
    descriptor.components.push_back({component_id.Value(), "astra.dialogue", 1, {{"line", "hello"}}});

    auto handle = world.Spawn(descriptor, diagnostics);
    REQUIRE(handle);
    REQUIRE(world.Activate(handle.Value(), diagnostics));
    REQUIRE(world.ActorCount() == 1);
    REQUIRE(Astra::Scene::FoundationComponentDescriptors().size() == 6);
    REQUIRE(world.ResolveActor(actor_id.Value(), diagnostics));
    auto component = world.FindComponent(handle.Value(), "astra.dialogue");
    REQUIRE(component);
    REQUIRE(component->data["line"] == "hello");
    REQUIRE(world.FindComponent(handle.Value(), "astra.blackboard"));
    REQUIRE(world.FindComponent(handle.Value(), "astra.control_policy"));

    auto snapshot = world.Snapshot();
    REQUIRE(snapshot.actors.size() == 1);
    REQUIRE(Astra::Scene::ToJson(snapshot).dump().find("entt") == std::string::npos);

    REQUIRE(world.Destroy(handle.Value(), diagnostics));
    REQUIRE_FALSE(world.IsHandleAlive(handle.Value()));
    REQUIRE_FALSE(world.Destroy(handle.Value(), diagnostics));
    REQUIRE_FALSE(world.ResolveActor(actor_id.Value(), diagnostics));
}



TEST_CASE("Scene headless local ECS pack syncs snapshots without exposing entities") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Scene::ActorWorld world;
    auto actor_id = Astra::Core::ParseStableId("actor:/ecs/one");
    auto actor_type = Astra::Core::ParseStableId("type:/astra.test.actor");
    REQUIRE(actor_id);
    REQUIRE(actor_type);
    auto handle = world.Spawn({actor_id.Value(), actor_type.Value(), "ECS One", {}}, diagnostics);
    REQUIRE(handle);

    auto pack = Astra::Scene::CreateHeadlessLocalEcsPack();
    pack->SyncIn({world.Snapshot().actors});
    pack->Update(1.0 / 60.0);
    auto output = pack->SyncOut();
    REQUIRE(output.actors.size() == 1);
    REQUIRE(output.emitted_events.size() == 1);
    Astra::Scene::WorldSnapshot output_snapshot;
    output_snapshot.actors = output.actors;
    REQUIRE(Astra::Scene::ToJson(output_snapshot).dump().find("entity") == std::string::npos);
}



TEST_CASE("Scene foundation stress spawns snapshots and destroys one thousand actors") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Scene::ActorWorld world;
    std::vector<Astra::Scene::ActorHandle> handles;
    auto actor_type = Astra::Core::ParseStableId("type:/astra.test.actor");
    REQUIRE(actor_type);

    for (int index = 0; index < 1000; ++index) {
        auto actor_id = Astra::Core::ParseStableId("actor:/stress/" + std::to_string(index));
        REQUIRE(actor_id);
        Astra::Scene::ActorDescriptor descriptor;
        descriptor.id = actor_id.Value();
        descriptor.type_id = actor_type.Value();
        descriptor.name = "Stress";
        auto handle = world.Spawn(descriptor, diagnostics);
        REQUIRE(handle);
        handles.push_back(handle.Value());
    }
    REQUIRE(world.ActorCount() == 1000);
    REQUIRE(world.Snapshot().actors.size() == 1000);
    for (const auto& handle : handles) {
        REQUIRE(world.Destroy(handle, diagnostics));
    }
    REQUIRE(world.ActorCount() == 0);
}



