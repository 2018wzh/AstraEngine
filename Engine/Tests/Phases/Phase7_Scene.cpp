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

TEST_CASE("Scene Phase 5 lifecycle supports deferred destroy preview attach and component migration") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Scene::ActorWorld world;
    auto actor_id = Astra::Core::ParseStableId("actor:/phase5/alice");
    auto actor_type = Astra::Core::ParseStableId("type:/astra.vn.character");
    auto component_id = Astra::Core::ParseStableId("component:/astra.phase5.profile");
    REQUIRE(actor_id);
    REQUIRE(actor_type);
    REQUIRE(component_id);

    Astra::Scene::ActorDescriptor descriptor;
    descriptor.id = actor_id.Value();
    descriptor.type_id = actor_type.Value();
    descriptor.name = "Alice";
    descriptor.components.push_back(
        {component_id.Value(), "astra.phase5.profile", 1, {{"displayName", "Alice"}, {"old_field", true}}});

    auto preview = world.PreviewAttach(descriptor, diagnostics);
    REQUIRE(preview);
    auto preview_snapshot = world.SnapshotActor(preview.Value());
    REQUIRE(preview_snapshot);
    REQUIRE(preview_snapshot->preview);
    REQUIRE(world.PreviewDetach(preview.Value(), diagnostics));
    REQUIRE(world.ActorCount() == 0);

    auto handle = world.Spawn(descriptor, diagnostics);
    REQUIRE(handle);
    Astra::Scene::ComponentMigrationStep step;
    step.component_type = "astra.phase5.profile";
    step.from_version = 1;
    step.to_version = 2;
    step.renamed_fields = {{"displayName", "display_name"}};
    step.default_data = {{"route_role", "heroine"}};
    step.deprecated_fields = {"old_field"};
    auto migration = world.MigrateComponent(handle.Value(), step, diagnostics);
    REQUIRE(migration);
    REQUIRE(migration.Value().changed);
    auto migrated = world.FindComponent(handle.Value(), "astra.phase5.profile");
    REQUIRE(migrated);
    REQUIRE(migrated->version == 2);
    REQUIRE(migrated->data["display_name"] == "Alice");
    REQUIRE(migrated->data["route_role"] == "heroine");
    REQUIRE_FALSE(migrated->data.contains("old_field"));

    REQUIRE(world.RequestDestroy(handle.Value(), diagnostics));
    REQUIRE(world.ActorCount() == 1);
    auto flushed = world.FlushDeferredDestroy(diagnostics);
    REQUIRE(flushed);
    REQUIRE(flushed.Value() == 1);
    REQUIRE(world.ActorCount() == 0);
}

TEST_CASE("Scene Phase 5 prefab overrides diff and rollback through DTOs") {
    Astra::Core::DiagnosticSink diagnostics;
    auto base_id = Astra::Core::ParseStableId("actor:/prefabs/alice_base");
    auto variant_id = Astra::Core::ParseStableId("actor:/prefabs/alice_school");
    auto actor_type = Astra::Core::ParseStableId("type:/astra.vn.character");
    auto transform_id = Astra::Core::ParseStableId("component:/astra.transform2d");
    REQUIRE(base_id);
    REQUIRE(variant_id);
    REQUIRE(actor_type);
    REQUIRE(transform_id);

    Astra::Scene::PrefabDescriptor base;
    base.id = base_id.Value();
    base.actor_type = actor_type.Value();
    base.name = "Alice";
    base.components.push_back({transform_id.Value(), "astra.transform2d", 1, {{"x", 0.0}, {"y", 0.0}}});

    Astra::Scene::PrefabDescriptor variant;
    variant.id = variant_id.Value();
    variant.actor_type = actor_type.Value();
    variant.name = "Alice School";
    variant.base = base_id.Value();
    variant.overrides.push_back({"astra.transform2d", "/x", 120.0});

    auto built = Astra::Scene::BuildActorFromPrefab(variant, &base, diagnostics);
    REQUIRE(built);
    REQUIRE(built.Value().actor.components.size() == 1);
    REQUIRE(built.Value().actor.components[0].data["x"] == 120.0);

    Astra::Scene::ActorSnapshot before;
    before.id = built.Value().actor.id;
    before.type_id = built.Value().actor.type_id;
    before.name = built.Value().actor.name;
    before.components = built.Value().actor.components;
    auto after = before;
    after.components[0].data["x"] = 240.0;
    auto diff = Astra::Scene::DiffPrefabInstance(before, after);
    REQUIRE(diff.size() == 1);

    auto rolled_back = Astra::Scene::RollbackPrefabInstance(after, variant, &base, diagnostics);
    REQUIRE(rolled_back);
    REQUIRE(rolled_back.Value().components[0].data["x"] == 120.0);
}



