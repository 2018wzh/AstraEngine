TEST_CASE("Script Phase 8 compiles production Astra DSL into IR source map and debug symbols") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Script::ScriptRuntimeHost host;
    Astra::Script::ScriptSource source;
    source.source_id = "native:/Scripts/opening";
    source.file = "opening.astra";
    source.text = R"(story prologue:
  state alice_route:
    scene station: #@id scene_station
      stage: #@id stage_station
        background native:/Backgrounds/Room #@id cmd_stage_bg
      background native:/Backgrounds/Room #@id cmd_bg
      show alice pose:normal native:/Characters/Alice/Normal at:center #@id cmd_show
      alice[normal]: Good morning from Phase 8. #@id line_001
      timeline 900ms: #@id timeline_intro
        camera zoom to:1.08 over:700ms #@id cmd_camera
      @live2d.motion.play actor:alice motion:greet mix:200ms #@id ext_motion
      choice "Walk?": #@id choice_station
        - "Walk together" -> route_walk #@id choice_walk
      -> route_walk #@id trans_walk
    scene route_walk: #@id scene_route_walk
      set route = walk #@id cmd_route
      end #@id end_route
)";

    auto compiled = host.CompileNative(source, diagnostics);
    REQUIRE(compiled);
    REQUIRE(compiled.Value().labels.contains("station"));
    REQUIRE(compiled.Value().labels.contains("route_walk"));
    REQUIRE_FALSE(compiled.Value().document.nodes.empty());
    REQUIRE_FALSE(compiled.Value().state_graph.scenes.empty());
    REQUIRE_FALSE(compiled.Value().narrative.commands.empty());
    REQUIRE_FALSE(compiled.Value().effects.effects.empty());
    REQUIRE_FALSE(compiled.Value().source_map.entries.empty());
    REQUIRE_FALSE(compiled.Value().debug_symbols.empty());
    REQUIRE_FALSE(compiled.Value().command_manifest.empty());
    REQUIRE(compiled.Value().commands[4].schema_id == "astra.vn.background.show");
    REQUIRE_FALSE(compiled.Value().extension_manifest.empty());
}

TEST_CASE("Script Phase 8 rejects missing stable ids and implicit scene fallthrough") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Script::ScriptRuntimeHost host;
    Astra::Script::ScriptSource source;
    source.source_id = "native:/Scripts/bad";
    source.file = "bad.astra";
    source.text = R"(story prologue:
  state route:
    scene station: #@id scene_station
      alice: Missing id.
    scene next: #@id scene_next
      end #@id end_next
)";

    auto compiled = host.CompileNative(source, diagnostics);
    REQUIRE_FALSE(compiled);
    REQUIRE(diagnostics.HasBlocking());
    auto text = nlohmann::json::array();
    for (const auto& diagnostic : diagnostics.Diagnostics()) {
        text.push_back(diagnostic.code);
    }
    REQUIRE(text.dump().find("ASTRA_SCRIPT_STABLE_ID_REQUIRED") != std::string::npos);
    REQUIRE(text.dump().find("ASTRA_SCRIPT_SCENE_FALLTHROUGH") != std::string::npos);
}

TEST_CASE("Script Phase 8 Lua extension sandbox registers schemas and blocks forbidden APIs") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Script::ScriptRuntimeHost host;
    Astra::Script::ScriptSource lua_source;
    lua_source.runtime_id = Astra::Script::LuaRuntimeId;
    lua_source.source_id = "native:/Scripts/live2d_extension";
    lua_source.file = "opening.lua";
    lua_source.text = R"(aivn.extension("live2d", "1.0.0")
aivn.command("motion.play", {
  version = 1,
  params = { actor = { type = "ActorRef", required = true } },
  execution = { deterministic = true, save = "serializable", skip = "finish", rollback = "snapshot" },
  editor = { label = "Motion" }
})
)";

    auto schemas = host.CompileLuaExtensionPackage(lua_source, diagnostics);
    REQUIRE(schemas);
    REQUIRE(schemas.Value().size() == 1);
    REQUIRE(schemas.Value()[0].extension_id == "live2d");

    Astra::Core::DiagnosticSink story_diagnostics;
    auto story = host.CompileLua(lua_source, story_diagnostics);
    REQUIRE_FALSE(story);
    REQUIRE(story_diagnostics.HasBlocking());

    Astra::Core::DiagnosticSink bad_diagnostics;
    lua_source.text = R"(io.open("x", "w"))";
    auto bad = host.CompileLuaExtensionPackage(lua_source, bad_diagnostics);
    REQUIRE_FALSE(bad);
    REQUIRE(bad_diagnostics.HasBlocking());
}

TEST_CASE("Script Phase 8 execution snapshot debug step and hot reload are deterministic") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Script::ScriptRuntimeHost host;
    Astra::Script::ScriptSource source;
    source.source_id = "native:/Scripts/opening";
    source.file = "opening.astra";
    source.text = R"(story prologue:
  state route:
    scene station: #@id scene_station
      background native:/Backgrounds/Room #@id cmd_bg
      alice: Hello. #@id line_001
      -> done #@id trans_done
    scene done: #@id scene_done
      end #@id end_done
)";
    auto compiled = host.CompileNative(source, diagnostics);
    REQUIRE(compiled);

    Astra::Runtime::RuntimeWorld runtime(7);
    auto result = host.Run(compiled.Value(), runtime, {"station", 0}, diagnostics);
    REQUIRE(result.snapshot.schema == Astra::Script::ScriptSnapshotV2Schema);
    REQUIRE(result.snapshot.active_scene == "done");
    REQUIRE(result.snapshot.stage_state["background"] == "native:/Backgrounds/Room");

    Astra::Script::ScriptSnapshot step_snapshot;
    step_snapshot.current_command_index = 1;
    auto step = host.Step(compiled.Value(), step_snapshot, runtime, diagnostics);
    REQUIRE(step.paused);
    REQUIRE_FALSE(step.current_command.command_id.empty());

    auto compatible = host.CheckHotReloadCompatibility(compiled.Value(), compiled.Value(), result.snapshot);
    REQUIRE(compatible.compatible);
    auto hot_reload = host.HotReload(compiled.Value(), compiled.Value(), result.snapshot);
    REQUIRE(hot_reload.reloaded);
    REQUIRE_FALSE(hot_reload.rolled_back);
}
