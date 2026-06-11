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



