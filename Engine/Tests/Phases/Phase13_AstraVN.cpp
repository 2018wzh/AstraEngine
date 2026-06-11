TEST_CASE("AstraVN foundation presets drive actor state machines without native handles") {
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
    source.text = R"(label opening
say alice "Testing."
choice "Continue" -> done
label done
)";
    auto result = session.RunNative(source, {"opening", 0}, diagnostics);
    REQUIRE(result);

    auto dialogue_id = Astra::Core::ParseStableId("actor:/systems/dialogue");
    REQUIRE(dialogue_id);
    auto handle = session.Runtime().Scene().ResolveActor(dialogue_id.Value(), diagnostics);
    REQUIRE(handle);
    auto state = session.Runtime().Scene().FindComponent(handle.value(), "astra.state_machine");
    REQUIRE(state);
    REQUIRE(state->data["current_state"] == "waiting_choice");
}



