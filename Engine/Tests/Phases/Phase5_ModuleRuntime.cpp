TEST_CASE("Module descriptor validation and dependency resolver emit blocking diagnostics") {
    Astra::ModuleRuntime::ModuleDescriptor module;
    module.id = "a";
    module.entrypoint = "Bin/a.dll";
    module.required_dependencies = {"missing"};

    Astra::ModuleRuntime::PluginDescriptor descriptor;
    descriptor.id = "astra.plugin.test";
    descriptor.version = "0.1.0";
    descriptor.astra_api = ">=0.1 <0.2";
    descriptor.modules = {module};
    Astra::Core::DiagnosticSink diagnostics;
    auto order = Astra::ModuleRuntime::ResolveModuleOrder(descriptor, diagnostics);
    REQUIRE_FALSE(order);
    REQUIRE(diagnostics.HasBlocking());
}

TEST_CASE("Module release gate validates descriptor policy and binary evidence") {
    Astra::ModuleRuntime::PluginDescriptor descriptor;
    descriptor.id = "astra.plugin.release_gate";
    descriptor.version = "0.1.0";
    descriptor.astra_api = ">=0.1 <0.2";
    descriptor.packaged_eligible = true;
    descriptor.diagnostics_code_prefix = "ASTRA_PLUGIN_RELEASE";
    Astra::ModuleRuntime::ModuleDescriptor module;
    module.id = "release.runtime";
    module.type = "runtime";
    module.load_phase = "runtime_startup";
    module.entrypoint = "Bin/missing.dll";
    module.packaged = true;
    module.permissions = {"runtime.packaged"};
    module.capabilities = {"service_provider"};
    descriptor.modules = {module};

    Astra::Core::DiagnosticSink diagnostics;
    auto report = Astra::ModuleRuntime::ValidateModuleReleaseGate(
        descriptor, std::filesystem::temp_directory_path(), diagnostics);
    REQUIRE_FALSE(report);
    REQUIRE(diagnostics.HasBlocking());
}

TEST_CASE("Service extension and provider registries reject duplicates") {
    Astra::ModuleRuntime::ServiceRegistry services;
    REQUIRE(
        services.Register({"service", "module", "capability", "v1", "engine", {"project_read"}}));
    Astra::ModuleRuntime::RegisteredService duplicate_service;
    duplicate_service.service_id = "service";
    duplicate_service.provider_module = "module2";
    duplicate_service.capability = "capability";
    REQUIRE_FALSE(services.Register(std::move(duplicate_service)));
    Astra::Core::DiagnosticSink diagnostics;
    auto denied = services.Resolve({"consumer",
                                    "service",
                                    "v1",
                                    {"capability"},
                                    {},
                                    Astra::ModuleRuntime::ModuleState::Active},
                                   diagnostics);
    REQUIRE_FALSE(denied);
    REQUIRE(diagnostics.HasBlocking());
    diagnostics.Clear();
    auto allowed = services.Resolve({"consumer",
                                     "service",
                                     "v1",
                                     {"capability"},
                                     {"project_read"},
                                     Astra::ModuleRuntime::ModuleState::Active},
                                    diagnostics);
    REQUIRE(allowed);
    REQUIRE(Astra::ModuleRuntime::ToJson(allowed.Value())["allowed"] == true);

    Astra::ModuleRuntime::ExtensionRegistry extensions;
    REQUIRE(extensions.Register({"extension", "module", "Kind"}));
    REQUIRE_FALSE(extensions.Register({"extension", "module", "Kind"}));

    Astra::ModuleRuntime::EngineModuleRegistry providers;
    REQUIRE(providers.RegisterSlot({"slot", "provider"}));
    REQUIRE(providers.RegisterProvider({"slot", "provider", "module"}));
    REQUIRE_FALSE(providers.RegisterProvider({"slot", "provider", "module"}));
    REQUIRE(providers.ValidatePolicy({{{"slot", "provider"}}}, diagnostics));
    REQUIRE_FALSE(providers.ValidatePolicy({{{"other", "provider"}}}, diagnostics));
}

TEST_CASE("Module manager reports ABI failures for real invalid binaries") {
#ifndef ASTRA_PHASE1_INVALID_PLUGIN_ROOT
    SKIP("Invalid ABI fixture plugins are not part of this build.");
#else
    auto platform = Astra::Platform::CreateHeadlessPlatform();

    Astra::ModuleRuntime::PluginDescriptor no_entry;
    no_entry.id = "astra.plugin.invalid.no_entry";
    no_entry.version = "0.1.0";
    no_entry.astra_api = ">=0.1 <0.2";
    no_entry.diagnostics_code_prefix = "ASTRA_INVALID_NO_ENTRY";
    Astra::ModuleRuntime::ModuleDescriptor no_entry_module;
    no_entry_module.id = "invalid.no_entry";
    no_entry_module.type = "runtime";
    no_entry_module.load_phase = "runtime_startup";
    no_entry_module.entrypoint = std::string("Bin/win64/") + ASTRA_PHASE1_INVALID_NO_ENTRY;
    no_entry.modules.push_back(std::move(no_entry_module));
    Astra::Core::DiagnosticSink no_entry_diagnostics;
    Astra::ModuleRuntime::ModuleManager no_entry_manager(platform);
    REQUIRE_FALSE(no_entry_manager.LoadAndActivate(no_entry, ASTRA_PHASE1_INVALID_PLUGIN_ROOT,
                                                   no_entry_diagnostics));
    REQUIRE(no_entry_diagnostics.HasBlocking());

    Astra::ModuleRuntime::PluginDescriptor bad_abi;
    bad_abi.id = "astra.plugin.invalid.bad_abi";
    bad_abi.version = "0.1.0";
    bad_abi.astra_api = ">=0.1 <0.2";
    bad_abi.diagnostics_code_prefix = "ASTRA_INVALID_BAD_ABI";
    Astra::ModuleRuntime::ModuleDescriptor bad_abi_module;
    bad_abi_module.id = "invalid.bad_abi";
    bad_abi_module.type = "runtime";
    bad_abi_module.load_phase = "runtime_startup";
    bad_abi_module.entrypoint = std::string("Bin/win64/") + ASTRA_PHASE1_INVALID_BAD_ABI;
    bad_abi.modules.push_back(std::move(bad_abi_module));
    Astra::Core::DiagnosticSink bad_abi_diagnostics;
    Astra::ModuleRuntime::ModuleManager bad_abi_manager(platform);
    REQUIRE_FALSE(bad_abi_manager.LoadAndActivate(bad_abi, ASTRA_PHASE1_INVALID_PLUGIN_ROOT,
                                                  bad_abi_diagnostics));
    REQUIRE(bad_abi_diagnostics.HasBlocking());
#endif
}

TEST_CASE("Example foundation plugin loads registers and unloads through module manager") {
#ifndef ASTRA_PHASE1_PLUGIN_DESCRIPTOR
    SKIP("Example foundation plugin is not part of this build.");
#else
    const std::filesystem::path descriptor_path = ASTRA_PHASE1_PLUGIN_DESCRIPTOR;
    REQUIRE(std::filesystem::exists(descriptor_path));

    Astra::Core::DiagnosticSink diagnostics;
    auto descriptor = Astra::ModuleRuntime::LoadPluginDescriptor(descriptor_path, diagnostics);
    REQUIRE(descriptor);

    auto platform = Astra::Platform::CreateHeadlessPlatform();
    Astra::ModuleRuntime::ModuleManager manager(platform);
    auto loaded =
        manager.LoadAndActivate(descriptor.Value(), descriptor_path.parent_path(), diagnostics);
    REQUIRE(loaded);
    REQUIRE(manager.State("phase1.example.runtime") == Astra::ModuleRuntime::ModuleState::Active);
    REQUIRE(manager.Services().Find("astra.phase1.example.service") != nullptr);
    REQUIRE(manager.Extensions().Extensions().size() == 1);
    REQUIRE(manager.Extensions().Extensions()[0].kind == "AssetImporter");
    REQUIRE(manager.EngineModules().Providers().size() == 1);
    REQUIRE(manager.EngineModules().Providers()[0].slot_id == "astra.renderer2d");

    manager.DeactivateAndUnload(diagnostics);
    REQUIRE(manager.State("phase1.example.runtime") == Astra::ModuleRuntime::ModuleState::Unloaded);
#endif
}

#if defined(ASTRA_WITH_TOOLS)
