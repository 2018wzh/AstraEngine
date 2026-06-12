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


TEST_CASE("Asset importers validate requests and write production sidecars") {
    const auto root = std::filesystem::temp_directory_path() / "astra_phase6_import";
    std::filesystem::remove_all(root);
    std::filesystem::create_directories(root / "Source");
    const auto source = root / "Source/alice.png";
    {
        const auto png = TestPng1x1Rgba();
        std::ofstream file(source, std::ios::binary);
        file.write(reinterpret_cast<const char*>(png.data()), static_cast<std::streamsize>(png.size()));
    }

    auto asset_id = Astra::Asset::ParseAssetUri("native:/Characters/Alice/Normal");
    REQUIRE(asset_id);

    Astra::Asset::ImportRequest request;
    request.source_path = source;
    request.target_asset_id = asset_id.Value();
    request.asset_type = "image";
    request.preset = "sprite";
    request.license.owner = "project";
    request.license.usage = "internal";

    Astra::Core::DiagnosticSink diagnostics;
    REQUIRE(Astra::Asset::ValidateImportRequest(request, diagnostics));
    auto importer = Astra::Asset::CreateBuiltinImporter("astra.importer.image");
    REQUIRE(importer);
    auto descriptor = importer->Describe();
    REQUIRE(Astra::Asset::ValidateImporterDescriptor(descriptor, diagnostics));
    REQUIRE(std::ranges::find(descriptor.source_extensions, ".png") != descriptor.source_extensions.end());

    auto preview = importer->Preview(request, diagnostics);
    REQUIRE(preview);
    REQUIRE(preview.Value().metadata.value("size_bytes", 0ull) > 0);

    auto imported = importer->Import(request, root / "Content", diagnostics);
    REQUIRE(imported);
    REQUIRE(std::filesystem::exists(imported.Value().source_path));
    REQUIRE(std::filesystem::exists(imported.Value().sidecar_path));

    Astra::Asset::AssetRegistryBuilder builder;
    auto registry = builder.Scan(root / "Content", diagnostics);
    REQUIRE(registry.entries.size() == 1);
    REQUIRE(registry.entries[0].importer_id.empty());
    REQUIRE(registry.entries[0].id.ToString() == "native:/Characters/Alice/Normal");
    REQUIRE_FALSE(registry.entries[0].source_hash.empty());
    REQUIRE(Astra::Asset::ComputeAssetRegistryHash(registry).size() == 64);

    request.license.owner.clear();
    REQUIRE_FALSE(Astra::Asset::ValidateImportRequest(request, diagnostics));
}

TEST_CASE("Asset cooker reuses DDC recovers corruption and writes binary zstd packages") {
    const auto root = std::filesystem::temp_directory_path() / "astra_phase6_package";
    std::filesystem::remove_all(root);
    std::filesystem::create_directories(root / "Content/Characters");
    {
        const auto png = TestPng1x1Rgba();
        std::ofstream image(root / "Content/Characters/alice.png", std::ios::binary);
        image.write(reinterpret_cast<const char*>(png.data()), static_cast<std::streamsize>(png.size()));
    }
    {
        std::ofstream sidecar(root / "Content/Characters/alice.asset.yaml", std::ios::binary);
        sidecar << "id: native:/Characters/Alice/Normal\n";
        sidecar << "type: image\n";
        sidecar << "source_path: alice.png\n";
        sidecar << "display_name: Alice Normal\n";
        sidecar << "origin: HumanAuthored\n";
        sidecar << "license:\n  owner: project\n  usage: internal\n";
        sidecar << "review:\n  status: accepted\n";
        sidecar << "cook:\n  preset: sprite\n";
        sidecar << "dependencies:\n  hard: []\n  soft: []\n";
    }

    Astra::Core::DiagnosticSink diagnostics;
    Astra::Asset::AssetRegistryBuilder builder;
    auto registry = builder.Scan(root / "Content", diagnostics);
    REQUIRE(registry.entries.size() == 1);

    Astra::Asset::CookPipelineOptions options;
    options.project_id = "nativevn-test";
    options.profile = "deterministic";
    options.content_root = root / "Content";
    options.cooked_root = root / "Saved/Cooked";
    options.ddc_root = root / "Saved/DDC";
    options.selected_providers = {{"image", "astra.cook.image.texture"}};
    options.provider_feature_hash = Astra::Asset::ComputeProviderFeatureHash(options.selected_providers);

    auto cooked = Astra::Asset::CookAssetRegistry(registry, options, diagnostics);
    REQUIRE(cooked);
    REQUIRE(cooked.Value().artifacts.size() == 1);
    REQUIRE(cooked.Value().ddc_entries.size() == 1);
    REQUIRE(cooked.Value().ddc_entries[0].rebuilt);
    REQUIRE_FALSE(cooked.Value().ddc_entries[0].corruption_recovered);
    REQUIRE(std::filesystem::exists(cooked.Value().artifacts[0].artifact_path));
    REQUIRE(std::filesystem::exists(cooked.Value().ddc_entries[0].artifacts[0].path));

    auto reused = Astra::Asset::CookAssetRegistry(registry, options, diagnostics);
    REQUIRE(reused);
    REQUIRE_FALSE(reused.Value().ddc_entries[0].rebuilt);

    {
        std::ofstream corrupt(reused.Value().ddc_entries[0].artifacts[0].path, std::ios::binary | std::ios::trunc);
        corrupt << "corrupt cache payload";
    }
    auto recovered = Astra::Asset::CookAssetRegistry(registry, options, diagnostics);
    REQUIRE(recovered);
    REQUIRE(recovered.Value().ddc_entries[0].rebuilt);
    REQUIRE(recovered.Value().ddc_entries[0].corruption_recovered);
    const auto stale_cache = options.ddc_root / "cache/stale.bin";
    {
        std::filesystem::create_directories(stale_cache.parent_path());
        std::ofstream stale(stale_cache, std::ios::binary);
        stale << "stale cache payload";
    }
    auto clean = Astra::Asset::CleanDerivedDataCache(options.ddc_root, recovered.Value().ddc_entries, diagnostics);
    REQUIRE(clean.removed == 1);
    REQUIRE(clean.retained >= 1);
    REQUIRE_FALSE(std::filesystem::exists(stale_cache));
    REQUIRE(std::filesystem::exists(recovered.Value().ddc_entries[0].artifacts[0].path));
    REQUIRE(Astra::Asset::ToJson(clean)["schema"] == Astra::Asset::DdcCleanReportSchema);

    Astra::Asset::PackageManifest manifest;
    manifest.package_id = "package:/Phase6Package";
    manifest.profile = "deterministic";
    manifest.project_hash = Astra::Asset::ComputeAssetRegistryHash(registry);
    manifest.cook_manifest = recovered.Value();

    Astra::Asset::PackageWriter writer;
    auto written = writer.WritePackage(manifest, root / "Saved/Packages/Phase6.astrapkg", diagnostics);
    REQUIRE(written);
    REQUIRE(written.Value().payloads.size() == 1);
    REQUIRE(written.Value().payloads[0].encoding == "binary");
    REQUIRE(written.Value().payloads[0].compression == "zstd");
    REQUIRE(written.Value().payloads[0].offset > 0);

    Astra::Asset::PackageReader reader;
    auto read_manifest = reader.ReadManifest(root / "Saved/Packages/Phase6.astrapkg", diagnostics);
    REQUIRE(read_manifest);
    REQUIRE(read_manifest.Value().package_hash == written.Value().package_hash);
    auto bytes = reader.ReadPayloadBytes(root / "Saved/Packages/Phase6.astrapkg", registry.entries[0].id, diagnostics);
    REQUIRE(bytes);
    REQUIRE(bytes.Value() == TestPng1x1Rgba());
    auto chunks = reader.ReadPayloadChunks(root / "Saved/Packages/Phase6.astrapkg", registry.entries[0].id, 7, diagnostics);
    REQUIRE(chunks);
    REQUIRE(chunks.Value().size() > 1);
    REQUIRE(chunks.Value().back().final_chunk);
    auto mount = reader.MountPackage(root / "Saved/Packages/Phase6.astrapkg", diagnostics);
    REQUIRE(mount);
    REQUIRE(mount.Value().read_only);
    REQUIRE(mount.Value().policy.allow_random_access);
    REQUIRE(mount.Value().policy.allow_chunked_read);

    Astra::Asset::AssetReleaseGateRequest gate_request;
    gate_request.registry = registry;
    gate_request.cook_manifest = written.Value().cook_manifest;
    gate_request.package_manifest = written.Value();
    gate_request.profile = "deterministic";
    auto gate = Astra::Asset::ValidateAssetReleaseGate(gate_request, diagnostics);
    REQUIRE(gate.passed);
}

TEST_CASE("Asset release gate localizes blockers and hot reload rollback decisions") {
    const auto root = std::filesystem::temp_directory_path() / "astra_phase6_release_gate";
    std::filesystem::remove_all(root);
    std::filesystem::create_directories(root / "Content/Characters");
    {
        std::ofstream image(root / "Content/Characters/alice.png", std::ios::binary);
        image << "image payload";
    }
    {
        std::ofstream sidecar(root / "Content/Characters/alice.asset.yaml", std::ios::binary);
        sidecar << "id: native:/Characters/Alice/Normal\n";
        sidecar << "type: image\n";
        sidecar << "source_path: alice.png\n";
        sidecar << "display_name: Alice Normal\n";
        sidecar << "origin: HumanAuthored\n";
        sidecar << "license:\n  owner: project\n  usage: internal\n";
        sidecar << "review:\n  status: accepted\n";
        sidecar << "cook:\n  preset: sprite\n";
        sidecar << "dependencies:\n  hard: [native:/Characters/Missing]\n  soft: []\n";
    }

    Astra::Core::DiagnosticSink diagnostics;
    Astra::Asset::AssetRegistryBuilder builder;
    auto registry = builder.Scan(root / "Content", diagnostics);
    REQUIRE(registry.entries.size() == 1);
    REQUIRE(diagnostics.HasBlocking());
    REQUIRE(registry.entries[0].license.owner == "project");

    Astra::Asset::CookManifest cook_manifest;
    cook_manifest.project_id = "release-gate-test";
    cook_manifest.profile = "deterministic";
    Astra::Asset::CookArtifact artifact;
    artifact.asset_id = registry.entries[0].id;
    artifact.type = "image";
    artifact.artifact_path = root / "Saved/Cooked/Characters/Alice/Normal.astbin";
    artifact.hash = "expected";
    artifact.processor_id = "astra.cook.image.texture";
    artifact.ddc_key = "ddc:/bad";
    cook_manifest.artifacts.push_back(artifact);
    Astra::Asset::DerivedDataCacheEntry ddc;
    ddc.key = "ddc:/bad";
    ddc.output_hash = "actual";
    cook_manifest.ddc_entries.push_back(ddc);

    Astra::Asset::PackageManifest package_manifest;
    package_manifest.package_id = "package:/Bad";
    package_manifest.cook_manifest = cook_manifest;
    Astra::Asset::PackagePayloadEntry payload;
    payload.asset_id = registry.entries[0].id;
    payload.hash = "other";
    package_manifest.payloads.push_back(payload);

    Astra::Asset::AssetReleaseGateRequest request;
    request.registry = registry;
    auto invalid_license = registry.entries[0];
    invalid_license.id = Astra::Asset::ParseAssetUri("native:/Characters/InvalidLicense").Value();
    invalid_license.hard_dependencies.clear();
    invalid_license.diagnostics.clear();
    invalid_license.license = {};
    request.registry.entries.push_back(invalid_license);
    auto unreviewed_ai = registry.entries[0];
    unreviewed_ai.id = Astra::Asset::ParseAssetUri("native:/Characters/AiDraft").Value();
    unreviewed_ai.hard_dependencies.clear();
    unreviewed_ai.diagnostics.clear();
    unreviewed_ai.origin = "AIGenerated";
    unreviewed_ai.review.status = "pending";
    unreviewed_ai.has_ai_generation = true;
    request.registry.entries.push_back(unreviewed_ai);
    auto soft_required = registry.entries[0];
    soft_required.id = Astra::Asset::ParseAssetUri("native:/Characters/SoftRequired").Value();
    soft_required.hard_dependencies.clear();
    soft_required.diagnostics.clear();
    soft_required.soft_dependencies.push_back(Astra::Asset::ParseAssetUri("native:/Characters/SoftMissing").Value());
    request.registry.entries.push_back(soft_required);
    auto virtual_ref = registry.entries[0];
    virtual_ref.id = Astra::Asset::ParseAssetUri("native:/Characters/VirtualRef").Value();
    virtual_ref.hard_dependencies.clear();
    virtual_ref.diagnostics.clear();
    virtual_ref.soft_dependencies.push_back(Astra::Asset::ParseAssetUri("virtual:/current/character").Value());
    request.registry.entries.push_back(virtual_ref);
    auto missing_sidecar_duplicate = registry.entries[0];
    missing_sidecar_duplicate.hard_dependencies.clear();
    missing_sidecar_duplicate.diagnostics.clear();
    missing_sidecar_duplicate.sidecar_path = root / "Content/Characters/missing.asset.yaml";
    request.registry.entries.push_back(missing_sidecar_duplicate);
    auto foreign_asset = registry.entries[0];
    foreign_asset.id = Astra::Asset::ParseAssetUri("foreign-artemis:/Characters/Alice").Value();
    foreign_asset.hard_dependencies.clear();
    foreign_asset.diagnostics.clear();
    request.registry.entries.push_back(foreign_asset);
    request.cook_manifest = cook_manifest;
    package_manifest.modules.push_back({"unsafe.provider", "Unsafe.dll", "", false});
    request.package_manifest = package_manifest;
    request.profile = "deterministic";
    request.require_soft_dependencies = true;
    auto report = Astra::Asset::ValidateAssetReleaseGate(request, diagnostics);
    REQUIRE_FALSE(report.passed);
    const auto json = Astra::Asset::ToJson(report);
    REQUIRE(json.dump().find("ASTRA_RELEASE_ASSET_DUPLICATE_ID") != std::string::npos);
    REQUIRE(json.dump().find("ASTRA_RELEASE_ASSET_SIDECAR_MISSING") != std::string::npos);
    REQUIRE(json.dump().find("ASTRA_RELEASE_ASSET_DEPENDENCY_MISSING") != std::string::npos);
    REQUIRE(json.dump().find("ASTRA_RELEASE_ASSET_INVALID_LICENSE") != std::string::npos);
    REQUIRE(json.dump().find("ASTRA_RELEASE_ASSET_004") != std::string::npos);
    REQUIRE(json.dump().find("ASTRA_RELEASE_ASSET_FOREIGN_COPY") != std::string::npos);
    REQUIRE(json.dump().find("ASTRA_RELEASE_ASSET_SOFT_DEPENDENCY_MISSING") != std::string::npos);
    REQUIRE(json.dump().find("ASTRA_RELEASE_ASSET_VIRTUAL_UNRESOLVED") != std::string::npos);
    REQUIRE(json.dump().find("ASTRA_RELEASE_COOK_ARTIFACT_MISSING") != std::string::npos);
    REQUIRE(json.dump().find("ASTRA_RELEASE_COOK_FORMAT_UNSUPPORTED") != std::string::npos);
    REQUIRE(json.dump().find("ASTRA_RELEASE_DDC_HASH_MISMATCH") != std::string::npos);
    REQUIRE(json.dump().find("ASTRA_RELEASE_MODULE_NOT_PACKAGE_ELIGIBLE") != std::string::npos);
    REQUIRE(json.dump().find("ASTRA_RELEASE_PACKAGE_PAYLOAD_HASH_MISMATCH") != std::string::npos);

    auto fixed_registry = registry;
    fixed_registry.entries[0].hard_dependencies.clear();
    fixed_registry.entries[0].diagnostics.clear();
    fixed_registry.entries[0].source_hash += "-changed";
    auto transaction = Astra::Asset::PlanHotReloadTransaction(registry, fixed_registry, registry.entries[0].id, diagnostics);
    REQUIRE(transaction.stage == Astra::Asset::HotReloadStage::SwitchAtFrameBoundary);
    REQUIRE_FALSE(transaction.rollback_required);

    auto removed = Astra::Asset::AssetRegistry{};
    transaction = Astra::Asset::PlanHotReloadTransaction(registry, removed, registry.entries[0].id, diagnostics);
    REQUIRE(transaction.stage == Astra::Asset::HotReloadStage::RolledBack);
    REQUIRE(transaction.rollback_required);
}


