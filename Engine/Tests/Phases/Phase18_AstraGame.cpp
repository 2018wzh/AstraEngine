TEST_CASE("AstraGame reports missing packages as launch failures", "[phase18][game]") {
    Astra::Core::DiagnosticSink diagnostics;
    Astra::Game::GameSession session;
    Astra::Game::GameLaunchDesc desc;
    desc.package_path = std::filesystem::temp_directory_path() / "missing.astrapkg";

    auto launched = session.Launch(desc, diagnostics);
    REQUIRE_FALSE(launched);
    REQUIRE(diagnostics.HasBlocking());
}

TEST_CASE("AstraGame exposes backend names", "[phase18][game]") {
    REQUIRE(Astra::Game::ToString(Astra::Game::GameBackend::Headless) == "headless");
    REQUIRE(Astra::Game::GameBackendFromString("sdl") == Astra::Game::GameBackend::Sdl);
    REQUIRE(Astra::Game::GameBackendFromString("mobile") == Astra::Game::GameBackend::Mobile);
    REQUIRE(Astra::Game::GameBackendFromString("web") == Astra::Game::GameBackend::Web);
}
