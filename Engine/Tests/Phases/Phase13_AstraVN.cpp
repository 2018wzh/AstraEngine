TEST_CASE("AstraVN Phase 8 presets and production DSL populate VN runtime state") {
    Astra::Core::DiagnosticSink diagnostics;
    auto profile = Astra::AstraVN::FoundationProfile();
    REQUIRE(profile.event_schemas.size() >= 8);
    REQUIRE(profile.actor_presets.size() == 8);
    REQUIRE(profile.component_presets.size() >= 7);
    REQUIRE(profile.state_machines.size() >= 6);

    Astra::AstraVN::VnSession session(12);
    REQUIRE(session.Runtime().Scene().ActorCount() == profile.actor_presets.size());

    Astra::Script::ScriptSource source;
    source.source_id = "native:/Scripts/opening";
    source.file = "opening.astra";
    source.text = R"(story prologue:
  state route:
    scene station: #@id scene_station
      stage: #@id stage_station
        background native:/Backgrounds/Room #@id cmd_stage_bg
      background native:/Backgrounds/Room #@id cmd_bg
      show alice pose:normal native:/Characters/Alice/Normal at:center #@id cmd_show
      alice[normal]: Testing. #@id line_001
      timeline 500ms: #@id timeline_test
        camera zoom to:1.1 over:500ms #@id cmd_camera
      choice "Continue?": #@id choice_continue
        - "Continue" -> done #@id choice_done
      -> done #@id trans_done
    scene done: #@id scene_done
      end #@id end_done
)";
    auto result = session.RunNative(source, {"station", 0}, diagnostics);
    REQUIRE(result);

    auto snapshot = session.CaptureSnapshot(diagnostics);
    REQUIRE(snapshot.script_snapshot.schema == Astra::Script::ScriptSnapshotV2Schema);
    REQUIRE(snapshot.stage_state.contains("background"));
    REQUIRE(snapshot.timeline_state.contains("timeline_test"));
    REQUIRE(snapshot.choice_state.contains("selected"));
    REQUIRE_FALSE(snapshot.backlog.empty());

    auto dialogue_id = Astra::Core::ParseStableId("actor:/systems/dialogue");
    REQUIRE(dialogue_id);
    auto handle = session.Runtime().Scene().ResolveActor(dialogue_id.Value(), diagnostics);
    REQUIRE(handle);
    auto state = session.Runtime().Scene().FindComponent(handle.value(), "astra.state_machine");
    REQUIRE(state);
    REQUIRE(state->data["current_state"] == "waiting_choice");
}
