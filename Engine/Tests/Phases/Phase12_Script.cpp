TEST_CASE("Script foundation compiles Native DSL and reports actionable diagnostics") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Script::ScriptRuntimeHost host;
    Astra::Script::ScriptSource source;
    source.source_id = "native:/Scripts/opening";
    source.file = "opening.astra";
    source.text = R"(label opening
bg native:/Backgrounds/Room
show alice native:/Characters/Alice/Normal center
say alice "Good morning." voice native:/Voice/Alice/opening_001
choice "Walk together" -> route_walk
label route_walk
set route walk
)";

    auto compiled = host.CompileNative(source, diagnostics);
    REQUIRE(compiled);
    REQUIRE(compiled.Value().labels.contains("opening"));
    REQUIRE(compiled.Value().labels.contains("route_walk"));
    REQUIRE_FALSE(compiled.Value().debug_symbols.empty());

    Astra::Core::DiagnosticSink bad_diagnostics;
    source.text = "label opening\njump missing_route\n";
    auto bad = host.CompileNative(source, bad_diagnostics);
    REQUIRE_FALSE(bad);
    REQUIRE(bad_diagnostics.HasBlocking());
    REQUIRE(bad_diagnostics.Diagnostics()[0].source.line > 0);
    REQUIRE_FALSE(bad_diagnostics.Diagnostics()[0].suggested_fixes.empty());
}



TEST_CASE("Script foundation Native DSL and Lua produce equivalent VN presentation") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Script::ScriptExecutionOptions options;
    options.entry_label = "opening";
    options.choice_index = 0;

    Astra::Script::ScriptSource native_source;
    native_source.runtime_id = Astra::Script::NativeRuntimeId;
    native_source.source_id = "native:/Scripts/opening";
    native_source.file = "opening.astra";
    native_source.text = R"(label opening
bg native:/Backgrounds/Room
show alice native:/Characters/Alice/Normal center
say alice "Good morning from Phase 4." voice native:/Voice/Alice/opening_001
audio music native:/Music/opening_theme
filter native:/Filters/soft_vn
choice "Walk together" -> route_walk
label route_walk
set route walk
say alice "Let's go."
)";

    Astra::Script::ScriptSource lua_source;
    lua_source.runtime_id = Astra::Script::LuaRuntimeId;
    lua_source.source_id = "native:/Scripts/opening";
    lua_source.file = "opening.lua";
    lua_source.text = R"(astra.label("opening")
astra.bg("native:/Backgrounds/Room")
astra.show("alice", "native:/Characters/Alice/Normal", "center")
astra.say("alice", "Good morning from Phase 4.", "native:/Voice/Alice/opening_001")
astra.audio("music", "native:/Music/opening_theme")
astra.filter("native:/Filters/soft_vn")
astra.choice("Walk together", "route_walk")
astra.label("route_walk")
astra.set("route", "walk")
astra.say("alice", "Let's go.")
)";

    Astra::AstraVN::VnSession native_session(44);
    auto native_result = native_session.RunNative(native_source, options, diagnostics);
    REQUIRE(native_result);
    auto native_snapshot = native_session.CaptureSnapshot(diagnostics);

    Astra::AstraVN::VnSession lua_session(44);
    auto lua_result = lua_session.RunLua(lua_source, options, diagnostics);
    REQUIRE(lua_result);
    auto lua_snapshot = lua_session.CaptureSnapshot(diagnostics);

    REQUIRE(native_snapshot.headless_capture.render_hash == lua_snapshot.headless_capture.render_hash);
    REQUIRE(native_snapshot.headless_capture.text_hash == lua_snapshot.headless_capture.text_hash);
    REQUIRE(native_snapshot.headless_capture.audio_hash == lua_snapshot.headless_capture.audio_hash);
    REQUIRE(native_snapshot.headless_capture.filter_hash == lua_snapshot.headless_capture.filter_hash);
    REQUIRE(native_snapshot.script_snapshot.variables["route"] == "walk");
    REQUIRE(native_snapshot.vn_events.size() >= 6);
    REQUIRE(native_snapshot.dialogue_history.size() == 2);

    Astra::AstraVN::VnSession restored(44);
    REQUIRE(restored.Restore(native_snapshot, diagnostics));
    REQUIRE(restored.Runtime().Hashes().state_hash == native_snapshot.hashes.state_hash);
    REQUIRE(restored.Runtime().Hashes().event_hash == native_snapshot.hashes.event_hash);
}



