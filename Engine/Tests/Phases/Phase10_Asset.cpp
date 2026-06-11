TEST_CASE("Asset foundation parses URIs resolves VFS and validates descriptors") {
    auto native = Astra::Asset::ParseAssetUri("native:/Characters\\\\Alice/Normal");
    REQUIRE(native);
    REQUIRE(native.Value().ToString() == "native:/Characters/Alice/Normal");
    REQUIRE(native.Value().ToStableId().Kind() == Astra::Core::StableIdKind::Asset);

    auto package = Astra::Core::ParseStableId("package:/Characters/Alice/Normal");
    REQUIRE(package);
    REQUIRE(package.Value().Kind() == Astra::Core::StableIdKind::Asset);

    Astra::Core::DiagnosticSink diagnostics;
    Astra::Asset::Vfs vfs;
    REQUIRE(vfs.Mount({"low", "native", "low", Astra::Asset::VfsAccess::ReadOnly, 1}, diagnostics));
    REQUIRE(vfs.Mount({"high", "native", "high", Astra::Asset::VfsAccess::ReadWrite, 100}, diagnostics));
    auto resolved = vfs.Resolve(native.Value());
    REQUIRE(resolved);
    REQUIRE(resolved->mount.mount_id == "high");
    REQUIRE_FALSE(resolved->read_only);

    Astra::Asset::ImportPresetDescriptor preset;
    preset.id = "astra.import.sprite.character";
    preset.source_extensions = {".png", ".webp"};
    preset.asset_type = "image";
    REQUIRE(Astra::Asset::ValidateImportPreset(preset, diagnostics));

    Astra::Asset::ProjectTemplateDescriptor project_template;
    project_template.id = "astra.template.vn.standard";
    project_template.display_name = "Standard AstraVN";
    project_template.runtime_profile = "astra.vn.runtime";
    REQUIRE(Astra::Asset::ValidateProjectTemplate(project_template, diagnostics));

    REQUIRE(Astra::Asset::ValidateReviewQueueItem({"review:/asset/one", "asset_import", "pending", "Saved/Agent/Drafts/one.png", "native:/Characters/Alice/Normal"}, diagnostics));
}



TEST_CASE("Asset registry scans sidecars and reports broken dependencies") {
    const auto root = std::filesystem::temp_directory_path() / "astra_phase3_assets";
    std::filesystem::remove_all(root);
    std::filesystem::create_directories(root / "Characters");
    {
        std::ofstream image(root / "Characters/alice.png", std::ios::binary);
        image << "fake image bytes";
    }
    {
        std::ofstream sidecar(root / "Characters/alice.asset.yaml", std::ios::binary);
        sidecar << "id: native:/Characters/Alice/Normal\n";
        sidecar << "type: image\n";
        sidecar << "source_path: alice.png\n";
        sidecar << "display_name: Alice Normal\n";
        sidecar << "origin: HumanAuthored\n";
        sidecar << "license:\n  owner: project\n  usage: internal\n";
        sidecar << "review:\n  status: accepted\n";
        sidecar << "cook:\n  preset: sprite\n";
        sidecar << "dependencies:\n  hard: [native:/Backgrounds/Missing]\n";
    }

    Astra::Core::DiagnosticSink diagnostics;
    Astra::Asset::AssetRegistryBuilder builder;
    auto registry = builder.Scan(root, diagnostics);
    REQUIRE(registry.entries.size() == 1);
    REQUIRE(registry.entries[0].id.ToString() == "native:/Characters/Alice/Normal");
    REQUIRE_FALSE(registry.entries[0].source_hash.empty());
    REQUIRE(diagnostics.HasBlocking());
    REQUIRE(Astra::Asset::ToJson(registry)["entries"][0]["dependencies"]["hard"][0] == "native:/Backgrounds/Missing");

    auto platform = Astra::Platform::CreateHeadlessPlatform();
    auto invalidations = builder.WatchContent(platform.FileSystem(), root);
    REQUIRE(invalidations.empty());
}



