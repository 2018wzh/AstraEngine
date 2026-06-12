TEST_CASE("Runtime replay foundation produces stable hashes for same seed and input") {
    auto run_once = [] {
        Astra::Core::DiagnosticSink diagnostics;
        Astra::Runtime::RuntimeWorld runtime(7);
        auto event_type = Astra::Core::ParseStableId("event:/astra.test.ping");
        REQUIRE(event_type);
        Astra::Runtime::RuntimeEvent event;
        event.type = event_type.Value();
        event.payload = {{"value", 42}};
        REQUIRE(runtime.Emit(event, Astra::Runtime::RuntimeEventMode::Deferred, diagnostics));
        REQUIRE(runtime.Tick(diagnostics));
        return runtime.Hashes();
    };

    const auto first = run_once();
    const auto second = run_once();
    REQUIRE(first.state_hash == second.state_hash);
    REQUIRE(first.event_hash == second.event_hash);
    REQUIRE(first.presentation_hash == second.presentation_hash);
}

TEST_CASE("RuntimeStress Phase 5 repeats one thousand actor save load replay hashes") {
    auto run_once = [] {
        Astra::Core::DiagnosticSink diagnostics;
        Astra::Runtime::RuntimeWorld runtime(2026);
        auto actor_type = Astra::Core::ParseStableId("type:/astra.stress.actor");
        auto state_machine_id = Astra::Core::ParseStableId("state_machine:/stress");
        auto event_type = Astra::Core::ParseStableId("event:/astra.stress.advance");
        auto state_component = Astra::Core::ParseStableId("component:/astra.state_machine");
        REQUIRE(actor_type);
        REQUIRE(state_machine_id);
        REQUIRE(event_type);
        REQUIRE(state_component);

        Astra::Runtime::StateMachineDefinition definition;
        definition.id = state_machine_id.Value();
        definition.initial_state = "idle";
        definition.transitions.push_back({"idle", event_type.Value(), "advanced"});
        runtime.RegisterStateMachine(std::move(definition));

        for (int index = 0; index < 1000; ++index) {
            auto actor_id = Astra::Core::ParseStableId("actor:/runtime_stress/" + std::to_string(index));
            REQUIRE(actor_id);
            Astra::Scene::ActorDescriptor descriptor;
            descriptor.id = actor_id.Value();
            descriptor.type_id = actor_type.Value();
            descriptor.name = "StressActor";
            descriptor.components.push_back({
                state_component.Value(),
                "astra.state_machine",
                1,
                {{"state_machine_id", state_machine_id.Value().ToString()}, {"current_state", "idle"}},
            });
            auto actor = runtime.Scene().Spawn(descriptor, diagnostics);
            REQUIRE(actor);
            REQUIRE(runtime.Scene().Activate(actor.Value(), diagnostics));
            Astra::Runtime::RuntimeEvent event;
            event.type = event_type.Value();
            event.priority = static_cast<Astra::Core::u32>(index % 3);
            event.target = {"actor", actor_id.Value().ToString()};
            REQUIRE(runtime.Emit(event, Astra::Runtime::RuntimeEventMode::Queued, diagnostics));
        }

        Astra::Runtime::RuntimeTask wait_time;
        wait_time.wait.kind = Astra::Runtime::RuntimeWaitKind::Time;
        wait_time.wait.wake_frame = 2;
        REQUIRE(runtime.ScheduleTask(wait_time, diagnostics));
        REQUIRE(runtime.Tick(diagnostics));
        auto save = runtime.SaveV2(false);
        Astra::Runtime::RuntimeWorld loaded;
        REQUIRE(loaded.Load(save, diagnostics));
        REQUIRE(loaded.Scene().ActorCount() == 1000);
        REQUIRE(loaded.Hashes().state_hash == runtime.Hashes().state_hash);
        return loaded.Hashes();
    };

    const auto first = run_once();
    const auto second = run_once();
    REQUIRE(first.state_hash == second.state_hash);
    REQUIRE(first.event_hash == second.event_hash);
    REQUIRE(first.presentation_hash == second.presentation_hash);
}



