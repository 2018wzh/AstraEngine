TEST_CASE("Runtime world orders events advances state machine and saves loads") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Runtime::RuntimeWorld runtime(1234);

    auto actor_id = Astra::Core::ParseStableId("actor:/systems/dialogue");
    auto actor_type = Astra::Core::ParseStableId("type:/astra.vn.dialogue_system");
    auto state_machine_id = Astra::Core::ParseStableId("state_machine:/dialogue_box");
    auto event_type = Astra::Core::ParseStableId("event:/astra.vn.dialogue.say_requested");
    auto component_id = Astra::Core::ParseStableId("component:/astra.state_machine");
    REQUIRE(actor_id);
    REQUIRE(actor_type);
    REQUIRE(state_machine_id);
    REQUIRE(event_type);
    REQUIRE(component_id);

    Astra::Scene::ActorDescriptor descriptor;
    descriptor.id = actor_id.Value();
    descriptor.type_id = actor_type.Value();
    descriptor.name = "Dialogue System";
    descriptor.components.push_back({
        component_id.Value(),
        "astra.state_machine",
        1,
        {{"state_machine_id", state_machine_id.Value().ToString()}, {"current_state", "idle"}},
    });
    auto actor = runtime.Scene().Spawn(descriptor, diagnostics);
    REQUIRE(actor);
    REQUIRE(runtime.Scene().Activate(actor.Value(), diagnostics));

    Astra::Runtime::StateMachineDefinition definition;
    definition.id = state_machine_id.Value();
    definition.initial_state = "idle";
    definition.transitions.push_back({"idle", event_type.Value(), "typing"});
    runtime.RegisterStateMachine(std::move(definition));

    Astra::Runtime::RuntimeEvent event;
    event.type = event_type.Value();
    event.category = "story";
    event.source = {"script", "native:/Scripts/opening"};
    event.target = {"actor", actor_id.Value().ToString()};
    event.payload_schema = "astra.vn.say_requested.v1";
    event.payload = {{"speaker", "actor:/characters/alice"}};
    REQUIRE(runtime.Emit(event, Astra::Runtime::RuntimeEventMode::Queued, diagnostics));
    REQUIRE(runtime.Tick(diagnostics));

    auto state_machine = runtime.Scene().FindComponent(actor.Value(), "astra.state_machine");
    REQUIRE(state_machine);
    REQUIRE(state_machine->data["current_state"] == "typing");
    REQUIRE(runtime.Hashes().state_hash == runtime.CaptureSnapshot().hashes.state_hash);
    auto replay = runtime.CaptureReplay();
    REQUIRE(replay.schema == Astra::Runtime::ReplaySchema);
    REQUIRE(replay.events.size() == 1);

    auto save = runtime.Save();
    REQUIRE(save.schema == Astra::Runtime::SnapshotSchema);
    const auto save_json = Astra::Core::ToJson(save).dump();
    REQUIRE(save_json.find("entt") == std::string::npos);
    REQUIRE(save_json.find("entity") == std::string::npos);
    REQUIRE(save_json.find("Actor*") == std::string::npos);

    Astra::Runtime::RuntimeWorld loaded;
    REQUIRE(loaded.Load(save, diagnostics));
    REQUIRE(loaded.CaptureSnapshot().frame_index == 1);
    REQUIRE(loaded.Scene().ActorCount() == 1);
    REQUIRE(loaded.Hashes().state_hash == runtime.Hashes().state_hash);
}

TEST_CASE("Runtime control policy allows queues and rejects locked channels") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Runtime::RuntimeWorld runtime;
    auto actor_id = Astra::Core::ParseStableId("actor:/characters/alice");
    auto actor_type = Astra::Core::ParseStableId("type:/astra.vn.character");
    auto policy_component = Astra::Core::ParseStableId("component:/astra.control_policy");
    REQUIRE(actor_id);
    REQUIRE(actor_type);
    REQUIRE(policy_component);

    Astra::Scene::ActorDescriptor descriptor;
    descriptor.id = actor_id.Value();
    descriptor.type_id = actor_type.Value();
    descriptor.name = "Alice";
    descriptor.components.push_back({
        policy_component.Value(),
        "astra.control_policy",
        1,
        {{"owner", "story_script"}, {"locked_channels", {"pose"}}, {"queue_locked", true}},
    });
    auto handle = runtime.Scene().Spawn(descriptor, diagnostics);
    REQUIRE(handle);

    auto owner =
        runtime.EvaluateControlPolicy({actor_id.Value(), "pose", "story_script", 80}, diagnostics);
    REQUIRE(owner.decision == Astra::Runtime::ControlDecision::Allow);
    auto queued =
        runtime.EvaluateControlPolicy({actor_id.Value(), "pose", "runtime_ai", 40}, diagnostics);
    REQUIRE(queued.decision == Astra::Runtime::ControlDecision::Queue);

    auto bob_id = Astra::Core::ParseStableId("actor:/characters/bob");
    REQUIRE(bob_id);
    descriptor.id = bob_id.Value();
    descriptor.name = "Bob";
    descriptor.components[0].data = {
        {"owner", "story_script"}, {"locked_channels", {"pose"}}, {"queue_locked", false}};
    auto reject_handle = runtime.Scene().Spawn(descriptor, diagnostics);
    REQUIRE(reject_handle);
    auto rejected =
        runtime.EvaluateControlPolicy({descriptor.id, "pose", "runtime_ai", 40}, diagnostics);
    REQUIRE(rejected.decision == Astra::Runtime::ControlDecision::Reject);
    auto system =
        runtime.EvaluateControlPolicy({descriptor.id, "pose", "system", 100}, diagnostics);
    REQUIRE(system.decision == Astra::Runtime::ControlDecision::Allow);
}

TEST_CASE("Runtime Phase 5 orders events by priority and wakes serializable scheduled tasks") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Runtime::RuntimeWorld runtime(42);
    auto actor_id = Astra::Core::ParseStableId("actor:/systems/phase5");
    auto actor_type = Astra::Core::ParseStableId("type:/astra.test.system");
    auto state_machine_id = Astra::Core::ParseStableId("state_machine:/phase5_order");
    auto low_event = Astra::Core::ParseStableId("event:/astra.test.low");
    auto high_event = Astra::Core::ParseStableId("event:/astra.test.high");
    auto scheduled_event = Astra::Core::ParseStableId("event:/astra.test.scheduled");
    auto state_component = Astra::Core::ParseStableId("component:/astra.state_machine");
    REQUIRE(actor_id);
    REQUIRE(actor_type);
    REQUIRE(state_machine_id);
    REQUIRE(low_event);
    REQUIRE(high_event);
    REQUIRE(scheduled_event);
    REQUIRE(state_component);

    Astra::Scene::ActorDescriptor descriptor;
    descriptor.id = actor_id.Value();
    descriptor.type_id = actor_type.Value();
    descriptor.name = "Phase5";
    descriptor.components.push_back({
        state_component.Value(),
        "astra.state_machine",
        1,
        {{"state_machine_id", state_machine_id.Value().ToString()}, {"current_state", "idle"}},
    });
    auto actor = runtime.Scene().Spawn(descriptor, diagnostics);
    REQUIRE(actor);

    Astra::Runtime::StateMachineDefinition definition;
    definition.id = state_machine_id.Value();
    definition.initial_state = "idle";
    definition.transitions.push_back({"idle", low_event.Value(), "low", 0});
    definition.transitions.push_back({"idle", high_event.Value(), "high", 100});
    definition.transitions.push_back({"high", scheduled_event.Value(), "scheduled", 0});
    runtime.RegisterStateMachine(std::move(definition));

    Astra::Runtime::RuntimeEvent low;
    low.type = low_event.Value();
    low.priority = 1;
    low.target = {"actor", actor_id.Value().ToString()};
    REQUIRE(runtime.Emit(low, Astra::Runtime::RuntimeEventMode::Queued, diagnostics));
    Astra::Runtime::RuntimeEvent high;
    high.type = high_event.Value();
    high.priority = 10;
    high.target = {"actor", actor_id.Value().ToString()};
    REQUIRE(runtime.Emit(high, Astra::Runtime::RuntimeEventMode::Queued, diagnostics));

    Astra::Runtime::RuntimeTask task;
    task.owner = {"actor", actor_id.Value().ToString()};
    task.wait.kind = Astra::Runtime::RuntimeWaitKind::Time;
    task.wait.wake_frame = 1;
    task.emit_on_wake.type = scheduled_event.Value();
    task.emit_on_wake.target = {"actor", actor_id.Value().ToString()};
    REQUIRE(runtime.ScheduleTask(task, diagnostics));
    Astra::Runtime::RuntimeTickInput input;
    input.package_profile = "deterministic";
    auto frame = runtime.Tick(input, diagnostics);
    REQUIRE(frame);
    REQUIRE(frame.Value().schema == "astra.runtime.frame_result.v1");
    REQUIRE(frame.Value().fixed_steps_executed == 1);
    REQUIRE(frame.Value().scheduled_tasks_completed.size() == 1);

    auto state_machine = runtime.Scene().FindComponent(actor.Value(), "astra.state_machine");
    REQUIRE(state_machine);
    REQUIRE(state_machine->data["current_state"] == "scheduled");
    REQUIRE(runtime.Scheduler().tasks.size() == 1);
    REQUIRE(runtime.Scheduler().tasks[0].state == Astra::Runtime::RuntimeTaskState::Completed);

    auto subscription = runtime.Subscribe(high_event.Value(), "test");
    REQUIRE(subscription.active);
    REQUIRE(runtime.Unsubscribe(subscription.id, diagnostics));

    Astra::Runtime::DirectorArbitrationRequest director_request;
    director_request.channel = "choice";
    director_request.owner = "runtime_ai";
    director_request.conflicts = {"story_script"};
    auto director = runtime.Director();
    director.choice_locked = true;
    runtime.SetDirector(director);
    auto arbitration = runtime.ArbitrateDirector(director_request, diagnostics);
    REQUIRE(arbitration.decision == Astra::Runtime::ControlDecision::Queue);
    REQUIRE(Astra::Runtime::ToJson(runtime.Director())["arbitration_log"].size() == 1);
}

TEST_CASE("Runtime Phase 5 save container v2 compresses scheduler state and localizes replay "
          "mismatches") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Runtime::RuntimeWorld runtime(99);
    auto event_type = Astra::Core::ParseStableId("event:/astra.test.wait");
    REQUIRE(event_type);
    Astra::Runtime::RuntimeTask task;
    task.wait.kind = Astra::Runtime::RuntimeWaitKind::Event;
    task.wait.event_type = event_type.Value();
    task.continuation = {{"note", "waiting for event"}};
    REQUIRE(runtime.ScheduleTask(task, diagnostics));

    auto save = runtime.SaveV2(true);
    REQUIRE(save.schema == Astra::Runtime::SaveContainerV2Schema);
    REQUIRE_FALSE(save.sections.empty());
    REQUIRE(save.section_descriptors.size() == save.sections.size());
    REQUIRE(std::ranges::any_of(save.section_descriptors, [](const auto& section) {
        return section.section_id == "section:/scene/actors" && section.required;
    }));
    REQUIRE(std::ranges::any_of(save.section_descriptors, [](const auto& section) {
        return section.section_id == "section:/ai/committed_output" && !section.required;
    }));
    REQUIRE(
        std::ranges::any_of(save.sections, [](const auto& section) { return section.compressed; }));
    auto save_json = Astra::Runtime::ToJson(save);
    auto parsed = Astra::Runtime::SaveContainerV2FromJson(save_json);
    REQUIRE(parsed);

    Astra::Runtime::RuntimeWorld loaded;
    REQUIRE(loaded.Load(parsed.Value(), diagnostics));
    REQUIRE(loaded.Scheduler().tasks.size() == 1);
    REQUIRE(loaded.Scheduler().tasks[0].wait.kind == Astra::Runtime::RuntimeWaitKind::Event);

    auto comparison = Astra::Runtime::CompareReplayHashes({"a", "b", "c"}, {"x", "b", "z"});
    REQUIRE_FALSE(comparison.passed);
    REQUIRE(comparison.localized_mismatches.size() == 2);
    REQUIRE(Astra::Runtime::ToJson(comparison)["localized_mismatches"].size() == 2);

    Astra::Runtime::ReplayStream stream;
    stream.records.push_back({1, 2, "scheduler_wake", "hash", {{"task", "task:/runtime/1"}}});
    stream.checkpoints.push_back({1, loaded.Hashes()});
    REQUIRE(Astra::Runtime::ToJson(stream)["schema"] == Astra::Runtime::ReplayStreamSchema);
}
