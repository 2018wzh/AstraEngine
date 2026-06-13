#include <Astra/Asset/Asset.hpp>

#include "AssetInternal.hpp"

#include <algorithm>
#include <csetjmp>
#include <fstream>
#include <iterator>
#include <set>
#include <sstream>

namespace Astra::Asset {

std::vector<CookProcessorDescriptor> BuiltinCookProcessorDescriptors();

namespace {

bool IsSupportedCookArtifact(std::string_view processor_id, const nlohmann::json& metadata);

ImporterDescriptor DescriptorForImporter(std::string_view provider_id) {
    if (provider_id == "astra.importer.image") {
        return {"astra.importer.image", "IAssetImporter", {".png", ".jpg", ".jpeg", ".webp"}, {"image", "image.sprite", "image.background"}, AssetSidecarSchema, "ASTRA_IMPORT_IMAGE"};
    }
    if (provider_id == "astra.importer.audio") {
        return {"astra.importer.audio", "IAssetImporter", {".ogg", ".wav", ".flac", ".mp3"}, {"audio"}, AssetSidecarSchema, "ASTRA_IMPORT_AUDIO"};
    }
    if (provider_id == "astra.importer.font") {
        return {"astra.importer.font", "IAssetImporter", {".otf", ".ttf"}, {"font"}, AssetSidecarSchema, "ASTRA_IMPORT_FONT"};
    }
    if (provider_id == "astra.importer.text") {
        return {"astra.importer.text", "IAssetImporter", {".txt", ".yaml", ".json"}, {"text", "timeline"}, AssetSidecarSchema, "ASTRA_IMPORT_TEXT"};
    }
    if (provider_id == "astra.importer.filter_profile") {
        return {"astra.importer.filter_profile", "IAssetImporter", {".filter.yaml"}, {"filter_profile"}, AssetSidecarSchema, "ASTRA_IMPORT_FILTER"};
    }
    if (provider_id == "astra.importer.script") {
        return {"astra.importer.script", "IAssetImporter", {".astra", ".lua"}, {"script.native", "script.lua"}, AssetSidecarSchema, "ASTRA_IMPORT_SCRIPT"};
    }
    return {};
}

CookProcessorDescriptor DescriptorForProcessor(std::string_view provider_id) {
    if (provider_id == "astra.cook.image.texture") {
        return {"astra.cook.image.texture", "ICookProcessor", {"image", "image.sprite", "image.background"}, {"astra.texture.source", "astra.texture.rgba8"}, {"astra.image_decode"}, true, 1, "ASTRA_COOK_IMAGE"};
    }
    if (provider_id == "astra.cook.audio.stream") {
        return {"astra.cook.audio.stream", "ICookProcessor", {"audio"}, {"astra.audio.stream"}, {"astra.audio_decode"}, true, 1, "ASTRA_COOK_AUDIO"};
    }
    if (provider_id == "astra.cook.font.atlas") {
        return {"astra.cook.font.atlas", "ICookProcessor", {"font"}, {"astra.font.runtime"}, {"astra.text_layout"}, true, 1, "ASTRA_COOK_FONT"};
    }
    if (provider_id == "astra.cook.filter_profile") {
        return {"astra.cook.filter_profile", "ICookProcessor", {"filter_profile"}, {"astra.filter_profile.runtime"}, {}, true, 1, "ASTRA_COOK_FILTER"};
    }
    if (provider_id == "astra.cook.script.native") {
        return {"astra.cook.script.native", "ICookProcessor", {"script.native"}, {"astra.script.native.ir"}, {"astra.script.native"}, true, 1, "ASTRA_COOK_SCRIPT"};
    }
    if (provider_id == "astra.cook.script.lua") {
        return {"astra.cook.script.lua", "ICookProcessor", {"script.lua"}, {"astra.script.lua.bytecode"}, {"astra.script.lua"}, true, 1, "ASTRA_COOK_SCRIPT"};
    }
    if (provider_id == "astra.cook.timeline") {
        return {"astra.cook.timeline", "ICookProcessor", {"timeline", "text"}, {"astra.timeline.runtime"}, {}, true, 1, "ASTRA_COOK_TIMELINE"};
    }
    if (provider_id == "astra.cook.asset.binary") {
        return {"astra.cook.asset.binary", "ICookProcessor", {"asset"}, {"astra.asset.binary"}, {}, true, 1, "ASTRA_COOK_ASSET"};
    }
    return {};
}

class GenericAssetImporter final : public IAssetImporter {
public:
    explicit GenericAssetImporter(ImporterDescriptor descriptor) : descriptor_(std::move(descriptor)) {}

    [[nodiscard]] ImporterDescriptor Describe() const override { return descriptor_; }

    [[nodiscard]] Astra::Core::Result<ImportPreview> Preview(const ImportRequest& request, Astra::Core::DiagnosticSink& diagnostics) const override {
        auto validation = ValidateImportRequest(request, diagnostics);
        if (!validation) {
            return Astra::Core::Result<ImportPreview>::Failure(validation.Error(), validation.Message());
        }
        const auto extension = LowerExtension(request.source_path);
        if (std::ranges::find(descriptor_.source_extensions, extension) == descriptor_.source_extensions.end()) {
            diagnostics.Emit(MakeDiagnostic(descriptor_.diagnostics_prefix + std::string("_EXTENSION_UNSUPPORTED"), Astra::Core::DiagnosticSeverity::Blocking, "Importer does not support the source extension.", request.source_path));
            return Astra::Core::Result<ImportPreview>::Failure(Astra::Core::ErrorCode::Unsupported, "unsupported import extension");
        }
        ImportPreview preview;
        preview.importer = descriptor_;
        preview.request = request;
        preview.metadata = {
            {"source_extension", extension},
            {"size_bytes", static_cast<Astra::Core::u64>(std::filesystem::file_size(request.source_path))},
            {"source_hash", Sha256File(request.source_path)},
        };
        return Astra::Core::Result<ImportPreview>::Success(std::move(preview));
    }

    [[nodiscard]] Astra::Core::Result<ImportedAsset> Import(const ImportRequest& request, const std::filesystem::path& content_root, Astra::Core::DiagnosticSink& diagnostics) const override {
        auto preview = Preview(request, diagnostics);
        if (!preview) {
            return Astra::Core::Result<ImportedAsset>::Failure(preview.Error(), preview.Message());
        }
        const auto relative_asset_path = std::filesystem::path(NormalizePath(request.target_asset_id.path));
        const auto target_source = content_root / relative_asset_path.parent_path() / request.source_path.filename();
        const auto sidecar_path = target_source.parent_path() / (target_source.stem().string() + ".asset.yaml");
        if (request.copy_source) {
            std::filesystem::create_directories(target_source.parent_path());
            std::filesystem::copy_file(request.source_path, target_source, std::filesystem::copy_options::overwrite_existing);
        }
        AssetSidecar sidecar;
        sidecar.id = request.target_asset_id;
        sidecar.type = request.asset_type;
        sidecar.source_path = target_source.filename();
        sidecar.display_name = target_source.stem().string();
        sidecar.origin = request.origin == "user_imported" ? "HumanAuthored" : request.origin;
        sidecar.license = request.license;
        sidecar.review.status = request.review_state;
        sidecar.cook.preset = request.preset;
        sidecar.requires_review = request.review_state != "accepted";

        ImportedAsset imported;
        imported.sidecar = std::move(sidecar);
        imported.sidecar_path = sidecar_path;
        imported.source_path = target_source;
        imported.audit = {
            {"importer", descriptor_.provider_id},
            {"preview", preview.Value().metadata},
            {"source_hash", Sha256File(target_source)},
        };
        const auto sidecar_json = ToJson(imported.sidecar).dump(2);
        WriteBytes(sidecar_path, std::span<const Astra::Core::u8>(reinterpret_cast<const Astra::Core::u8*>(sidecar_json.data()), sidecar_json.size()));
        return Astra::Core::Result<ImportedAsset>::Success(std::move(imported));
    }

private:
    ImporterDescriptor descriptor_;
};

std::string ProcessorIdForType(std::string_view type) {
    if (type == "image" || type == "image.sprite" || type == "image.background") {
        return "astra.cook.image.texture";
    }
    if (type == "audio") {
        return "astra.cook.audio.stream";
    }
    if (type == "font") {
        return "astra.cook.font.atlas";
    }
    if (type == "filter_profile") {
        return "astra.cook.filter_profile";
    }
    if (type == "script.native") {
        return "astra.cook.script.native";
    }
    if (type == "script.lua") {
        return "astra.cook.script.lua";
    }
    if (type == "timeline" || type == "text") {
        return "astra.cook.timeline";
    }
    return "astra.cook.asset.binary";
}

class GenericCookProcessor final : public ICookProcessor {
public:
    explicit GenericCookProcessor(CookProcessorDescriptor descriptor) : descriptor_(std::move(descriptor)) {}

    [[nodiscard]] CookProcessorDescriptor Describe() const override { return descriptor_; }

    [[nodiscard]] Astra::Core::Result<CookArtifactDescriptor> Cook(const CookRequest& request, std::span<const Astra::Core::u8> source_bytes, Astra::Core::DiagnosticSink& diagnostics) const override {
        const auto requested_format = descriptor_.output_formats.empty() ? std::string() : descriptor_.output_formats.front();
        if (requested_format.empty() ||
            std::ranges::find(descriptor_.output_formats, requested_format) == descriptor_.output_formats.end()) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_COOK_FORMAT_UNSUPPORTED", Astra::Core::DiagnosticSeverity::Blocking, "Cook processor does not support the requested format."));
            return Astra::Core::Result<CookArtifactDescriptor>::Failure(Astra::Core::ErrorCode::Unsupported, "unsupported cook format");
        }
        CookArtifactDescriptor artifact;
        artifact.schema = CookArtifactDescriptorSchema;
        artifact.artifact_id = request.asset.id.ToString() + "::" + descriptor_.provider_id;
        artifact.asset_id = request.asset.id;
        artifact.format = descriptor_.output_formats.empty() ? "binary" : descriptor_.output_formats.front();
        artifact.ddc_key = DdcKey{DdcKeySchema, request.asset.id, request.asset.source_hash, request.asset.sidecar_hash, descriptor_.provider_id, descriptor_.version, request.target_platform, request.release_profile, request.provider_feature_hash}.ToString();
        artifact.payload_hash = Sha256Span(source_bytes);
        artifact.payload_size = static_cast<Astra::Core::u64>(source_bytes.size());
        artifact.runtime_dependencies = request.asset.hard_dependencies;
        artifact.metadata = {
            {"source_extension", LowerExtension(request.asset.source_path)},
            {"provider_feature_hash", request.provider_feature_hash},
            {"package_eligible", descriptor_.package_eligible},
        };
        if (request.asset.type == "image" || request.asset.type == "image.sprite" || request.asset.type == "image.background") {
            artifact.metadata["media_inspect"] = InspectImageCookMetadata(source_bytes, request.asset.source_path);
        } else if (request.asset.type == "font") {
            artifact.metadata["font_inspect"] = InspectFontCookMetadata(source_bytes);
        } else if (request.asset.type == "audio") {
            artifact.metadata["audio_inspect"] = InspectAudioCookMetadata(source_bytes, request.asset.source_path);
        }
        return Astra::Core::Result<CookArtifactDescriptor>::Success(std::move(artifact));
    }

private:
    CookProcessorDescriptor descriptor_;
};

std::filesystem::path CookedPathForAsset(const CookPipelineOptions& options, const AssetUri& asset) {
    return options.cooked_root / NormalizePath(asset.path + ".astbin");
}

std::filesystem::path DdcPathForKey(const std::filesystem::path& ddc_root, std::string_view key) {
    auto safe = std::string(key);
    std::replace(safe.begin(), safe.end(), ':', '_');
    std::replace(safe.begin(), safe.end(), '/', '_');
    return ddc_root / "cache" / (safe + ".bin");
}

bool IsSupportedCookArtifact(std::string_view processor_id, const nlohmann::json& metadata) {
    const auto processors = BuiltinCookProcessorDescriptors();
    const auto found = std::ranges::find_if(processors, [&](const CookProcessorDescriptor& descriptor) {
        return descriptor.provider_id == processor_id;
    });
    if (found == processors.end() || !found->package_eligible) {
        return false;
    }
    const auto format = metadata.value("format", std::string());
    return !format.empty() &&
           std::ranges::find(found->output_formats, format) != found->output_formats.end();
}

} // namespace

std::vector<ImporterDescriptor> BuiltinImporterDescriptors() {
    return {
        DescriptorForImporter("astra.importer.image"),
        DescriptorForImporter("astra.importer.audio"),
        DescriptorForImporter("astra.importer.font"),
        DescriptorForImporter("astra.importer.text"),
        DescriptorForImporter("astra.importer.filter_profile"),
        DescriptorForImporter("astra.importer.script"),
    };
}

std::vector<CookProcessorDescriptor> BuiltinCookProcessorDescriptors() {
    return {
        DescriptorForProcessor("astra.cook.image.texture"),
        DescriptorForProcessor("astra.cook.audio.stream"),
        DescriptorForProcessor("astra.cook.font.atlas"),
        DescriptorForProcessor("astra.cook.filter_profile"),
        DescriptorForProcessor("astra.cook.script.native"),
        DescriptorForProcessor("astra.cook.script.lua"),
        DescriptorForProcessor("astra.cook.timeline"),
        DescriptorForProcessor("astra.cook.asset.binary"),
    };
}

std::unique_ptr<IAssetImporter> CreateBuiltinImporter(std::string_view provider_id) {
    auto descriptor = DescriptorForImporter(provider_id);
    if (descriptor.provider_id.empty()) {
        return {};
    }
    return std::make_unique<GenericAssetImporter>(std::move(descriptor));
}

std::unique_ptr<ICookProcessor> CreateBuiltinCookProcessor(std::string_view provider_id) {
    auto descriptor = DescriptorForProcessor(provider_id);
    if (descriptor.provider_id.empty()) {
        return {};
    }
    return std::make_unique<GenericCookProcessor>(std::move(descriptor));
}

Astra::Core::Result<CookManifest> CookAssetRegistry(const AssetRegistry& registry, const CookPipelineOptions& options, Astra::Core::DiagnosticSink& diagnostics) {
    CookManifest manifest;
    manifest.project_id = options.project_id.empty() ? "package:/project" : options.project_id;
    manifest.profile = options.profile;
    std::filesystem::create_directories(options.cooked_root);
    std::filesystem::create_directories(options.ddc_root);

    for (const auto& entry : registry.entries) {
        if (!entry.diagnostics.empty()) {
            for (const auto& diagnostic : entry.diagnostics) {
                if (diagnostic.BlocksRelease()) {
                    diagnostics.Emit(diagnostic);
                    return Astra::Core::Result<CookManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "registry contains blocking diagnostics");
                }
            }
        }
        if (!std::filesystem::exists(entry.source_path)) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_COOK_SOURCE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Cook source asset is missing.", entry.source_path));
            return Astra::Core::Result<CookManifest>::Failure(Astra::Core::ErrorCode::NotFound, "cook source missing");
        }
        const auto processor_id = ProcessorIdForType(entry.type);
        auto processor = CreateBuiltinCookProcessor(processor_id);
        if (!processor) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_COOK_PROCESSOR_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "No cook processor is registered for the asset type.", entry.source_path));
            return Astra::Core::Result<CookManifest>::Failure(Astra::Core::ErrorCode::Unsupported, "cook processor missing");
        }
        CookRequest request;
        request.asset = entry;
        request.target_platform = options.target_platform;
        request.release_profile = options.profile;
        request.selected_providers = options.selected_providers;
        request.provider_feature_hash = options.provider_feature_hash.empty() ? ComputeProviderFeatureHash(options.selected_providers) : options.provider_feature_hash;

        const auto source_bytes = ReadBytes(entry.source_path);
        auto cooked = processor->Cook(request, source_bytes, diagnostics);
        if (!cooked) {
            return Astra::Core::Result<CookManifest>::Failure(cooked.Error(), cooked.Message());
        }

        const auto cooked_path = CookedPathForAsset(options, entry.id);
        const auto ddc_path = DdcPathForKey(options.ddc_root, cooked.Value().ddc_key);
        const auto existing_hash = std::filesystem::exists(ddc_path) ? Sha256File(ddc_path) : std::string();
        const bool corruption = !existing_hash.empty() && existing_hash != cooked.Value().payload_hash;
        const bool rebuild = existing_hash != cooked.Value().payload_hash;
        if (rebuild) {
            WriteBytes(ddc_path, source_bytes);
        }
        WriteBytes(cooked_path, source_bytes);

        CookArtifact artifact;
        artifact.asset_id = entry.id;
        artifact.type = entry.type;
        artifact.artifact_path = cooked_path;
        artifact.hash = cooked.Value().payload_hash;
        artifact.processor_id = processor_id;
        artifact.ddc_key = cooked.Value().ddc_key;
        artifact.metadata = cooked.Value().metadata;
        artifact.metadata["artifact_id"] = cooked.Value().artifact_id;
        artifact.metadata["format"] = cooked.Value().format;
        artifact.metadata["runtime_dependencies"] = nlohmann::json::array();
        for (const auto& dependency : cooked.Value().runtime_dependencies) {
            artifact.metadata["runtime_dependencies"].push_back(dependency.ToString());
        }
        manifest.artifacts.push_back(std::move(artifact));

        DerivedDataCacheEntry ddc;
        ddc.key = cooked.Value().ddc_key;
        ddc.processor_id = processor_id;
        ddc.input_hash = entry.source_hash;
        ddc.settings_hash = Sha256Text(processor_id + "|" + entry.sidecar_hash + "|" + options.profile + "|" + request.provider_feature_hash);
        ddc.output_hash = cooked.Value().payload_hash;
        ddc.platform = options.target_platform;
        ddc.profile = options.profile;
        ddc.engine_version = "0.2.0";
        ddc.rebuilt = rebuild;
        ddc.corruption_recovered = corruption;
        ddc.artifacts.push_back({ddc_path, cooked.Value().payload_hash, static_cast<Astra::Core::u64>(source_bytes.size())});
        manifest.ddc_entries.push_back(std::move(ddc));
    }
    manifest.manifest_hash = ComputeCookManifestHash(ToJson(manifest));
    return Astra::Core::Result<CookManifest>::Success(std::move(manifest));
}

DdcCleanReport CleanDerivedDataCache(const std::filesystem::path& ddc_root, const std::vector<DerivedDataCacheEntry>& live_entries, Astra::Core::DiagnosticSink& diagnostics) {
    DdcCleanReport report;
    report.root = ddc_root;
    const auto cache_root = ddc_root / "cache";
    if (!std::filesystem::exists(cache_root)) {
        return report;
    }

    std::set<std::filesystem::path> retained_paths;
    for (const auto& entry : live_entries) {
        for (const auto& artifact : entry.artifacts) {
            retained_paths.insert(std::filesystem::absolute(artifact.path).lexically_normal());
            retained_paths.insert(std::filesystem::absolute(ddc_root / artifact.path).lexically_normal());
        }
        if (!entry.key.empty()) {
            retained_paths.insert(std::filesystem::absolute(DdcPathForKey(ddc_root, entry.key)).lexically_normal());
        }
    }

    const auto absolute_cache = std::filesystem::absolute(cache_root).lexically_normal();
    for (const auto& item : std::filesystem::recursive_directory_iterator(cache_root)) {
        if (!item.is_regular_file()) {
            continue;
        }
        ++report.scanned;
        const auto candidate = std::filesystem::absolute(item.path()).lexically_normal();
        if (!candidate.generic_string().starts_with(absolute_cache.generic_string())) {
            auto diagnostic = MakeDiagnostic("ASTRA_DDC_CLEAN_PATH_ESCAPE", Astra::Core::DiagnosticSeverity::Blocking, "DDC clean candidate escaped the cache root.", item.path());
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
            continue;
        }
        if (retained_paths.contains(candidate)) {
            ++report.retained;
            continue;
        }
        std::error_code error;
        if (std::filesystem::remove(candidate, error)) {
            ++report.removed;
        } else if (error) {
            auto diagnostic = MakeDiagnostic("ASTRA_DDC_CLEAN_REMOVE_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "DDC cache artifact could not be removed.", item.path());
            diagnostic.context["error"] = error.message();
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
    }
    return report;
}

AssetReleaseGateReport ValidateAssetReleaseGate(const AssetReleaseGateRequest& request, Astra::Core::DiagnosticSink& diagnostics) {
    AssetReleaseGateReport report;
    std::set<std::string> ids;
    for (const auto& entry : request.registry.entries) {
        ids.insert(entry.id.ToString());
    }
    std::set<std::string> seen_ids;
    for (const auto& entry : request.registry.entries) {
        const auto id = entry.id.ToString();
        if (!seen_ids.insert(id).second) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_DUPLICATE_ID", Astra::Core::DiagnosticSeverity::Blocking, "Duplicate AssetId blocks release.", entry.sidecar_path);
            diagnostic.objects.push_back({"AssetId", id});
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
        if (entry.sidecar_path.empty() || !std::filesystem::exists(entry.sidecar_path)) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_SIDECAR_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Asset sidecar is missing.", entry.source_path);
            diagnostic.objects.push_back({"AssetId", id});
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
        if (entry.source_path.empty() || !std::filesystem::exists(entry.source_path)) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_SOURCE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Asset source is missing.", entry.source_path);
            diagnostic.objects.push_back({"AssetId", id});
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
        if (entry.license.owner.empty() || entry.license.usage.empty()) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_INVALID_LICENSE", Astra::Core::DiagnosticSeverity::Blocking, "Asset license owner and usage are required for release.", entry.sidecar_path);
            diagnostic.objects.push_back({"AssetId", id});
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
        if ((entry.requires_review || entry.origin == "AIGenerated" || entry.has_ai_generation) && entry.review.status != "accepted") {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_004", Astra::Core::DiagnosticSeverity::Blocking, "Unreviewed AI or review-required asset cannot be packaged.", entry.sidecar_path);
            diagnostic.objects.push_back({"AssetId", id});
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
        if (entry.id.scheme == AssetScheme::Virtual) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_VIRTUAL_UNRESOLVED", Astra::Core::DiagnosticSeverity::Blocking, "Virtual asset refs must be resolved before package release.", entry.sidecar_path);
            diagnostic.objects.push_back({"AssetId", id});
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
        if (entry.id.scheme == AssetScheme::Foreign && !request.allow_foreign_copy) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_FOREIGN_COPY", Astra::Core::DiagnosticSeverity::Blocking, "Foreign asset copies are not allowed for release.", entry.sidecar_path);
            diagnostic.objects.push_back({"AssetId", id});
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
        if (request.require_soft_dependencies && entry.soft_dependencies.empty()) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_SOFT_DEPENDENCY_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Soft dependencies are required for release.", entry.sidecar_path);
            diagnostic.objects.push_back({"AssetId", id});
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
        for (const auto& dependency : entry.hard_dependencies) {
            if (dependency.scheme == AssetScheme::Virtual || dependency.scheme_name == "virtual") {
                auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_VIRTUAL_UNRESOLVED", Astra::Core::DiagnosticSeverity::Blocking, "Virtual hard dependency must be resolved before package release.", entry.sidecar_path);
                diagnostic.objects.push_back({"AssetId", id});
                diagnostic.objects.push_back({"VirtualAssetId", dependency.ToString()});
                report.diagnostics.push_back(diagnostic);
                diagnostics.Emit(std::move(diagnostic));
            }
            if (!ids.contains(dependency.ToString())) {
                auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_DEPENDENCY_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Hard dependency is missing from release registry.", entry.sidecar_path);
                diagnostic.objects.push_back({"AssetId", id});
                diagnostic.objects.push_back({"Dependency", dependency.ToString()});
                report.diagnostics.push_back(diagnostic);
                diagnostics.Emit(std::move(diagnostic));
            }
        }
        for (const auto& dependency : entry.soft_dependencies) {
            if (dependency.scheme == AssetScheme::Virtual || dependency.scheme_name == "virtual") {
                auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_VIRTUAL_UNRESOLVED", Astra::Core::DiagnosticSeverity::Blocking, "Virtual soft dependency must be resolved before package release.", entry.sidecar_path);
                diagnostic.objects.push_back({"AssetId", id});
                diagnostic.objects.push_back({"VirtualAssetId", dependency.ToString()});
                report.diagnostics.push_back(diagnostic);
                diagnostics.Emit(std::move(diagnostic));
            }
            if (request.require_soft_dependencies && !ids.contains(dependency.ToString())) {
                auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_ASSET_SOFT_DEPENDENCY_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Release profile requires soft dependencies to resolve.", entry.sidecar_path);
                diagnostic.objects.push_back({"AssetId", id});
                diagnostic.objects.push_back({"MissingAssetId", dependency.ToString()});
                report.diagnostics.push_back(diagnostic);
                diagnostics.Emit(std::move(diagnostic));
            }
        }
    }

    std::map<std::string, DerivedDataCacheEntry> ddc_by_key;
    for (const auto& entry : request.cook_manifest.ddc_entries) {
        ddc_by_key[entry.key] = entry;
    }
    std::set<std::string> cooked_assets;
    for (const auto& artifact : request.cook_manifest.artifacts) {
        cooked_assets.insert(artifact.asset_id.ToString());
    }
    for (const auto& entry : request.registry.entries) {
        if (!cooked_assets.contains(entry.id.ToString())) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_COOK_ARTIFACT_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Registry asset has no cook artifact.", entry.sidecar_path);
            diagnostic.objects.push_back({"AssetId", entry.id.ToString()});
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
    }
    for (const auto& artifact : request.cook_manifest.artifacts) {
        if (artifact.ddc_key.empty() || artifact.hash.empty()) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_COOK_ARTIFACT_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Cook artifact is missing DDC key or hash.", artifact.artifact_path);
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
            continue;
        }
        if (!IsSupportedCookArtifact(artifact.processor_id, artifact.metadata) || !artifact.metadata.value("package_eligible", true)) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_COOK_FORMAT_UNSUPPORTED", Astra::Core::DiagnosticSeverity::Blocking, "Cook artifact format or processor is not package eligible.", artifact.artifact_path);
            diagnostic.objects.push_back({"ProcessorId", artifact.processor_id});
            diagnostic.objects.push_back({"AssetId", artifact.asset_id.ToString()});
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
        const auto found = ddc_by_key.find(artifact.ddc_key);
        if (found == ddc_by_key.end() || found->second.output_hash != artifact.hash) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_DDC_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Cook artifact hash does not match DDC metadata.", artifact.artifact_path);
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
    }
    for (const auto& module : request.package_manifest.modules) {
        if (!module.runtime_safe || module.sha256.empty()) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_MODULE_NOT_PACKAGE_ELIGIBLE", Astra::Core::DiagnosticSeverity::Blocking, "Package module is not runtime-safe or has no binary hash.", module.binary);
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
    }
    std::map<std::string, PackagePayloadEntry> payloads;
    for (const auto& payload : request.package_manifest.payloads) {
        payloads[payload.asset_id.ToString()] = payload;
    }
    for (const auto& artifact : request.cook_manifest.artifacts) {
        const auto found = payloads.find(artifact.asset_id.ToString());
        if (found == payloads.end()) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_PACKAGE_PAYLOAD_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Package payload is missing for cook artifact.", artifact.artifact_path);
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        } else if (found->second.hash != artifact.hash) {
            auto diagnostic = MakeDiagnostic("ASTRA_RELEASE_PACKAGE_PAYLOAD_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Package payload hash does not match cook artifact.", artifact.artifact_path);
            report.diagnostics.push_back(diagnostic);
            diagnostics.Emit(std::move(diagnostic));
        }
    }

    report.passed = report.diagnostics.empty();
    report.evidence = {
        {"profile", request.profile},
        {"asset_count", request.registry.entries.size()},
        {"artifact_count", request.cook_manifest.artifacts.size()},
        {"package_hash", request.package_manifest.package_hash},
    };
    return report;
}

HotReloadTransaction PlanHotReloadTransaction(const AssetRegistry& before, const AssetRegistry& after, const AssetUri& asset_id, Astra::Core::DiagnosticSink& diagnostics) {
    AssetRegistryBuilder builder;
    const auto invalidations = builder.ComputeInvalidations(before, after);
    HotReloadTransaction transaction;
    transaction.transaction_id = "hotreload:/" + Sha256Text(asset_id.ToString() + "|" + ComputeAssetRegistryHash(after));
    transaction.asset_id = asset_id;
    const auto changed = std::ranges::find_if(invalidations, [&](const AssetInvalidation& invalidation) {
        return std::ranges::any_of(invalidation.affected_assets, [&](const AssetUri& affected) {
            return affected.ToString() == asset_id.ToString();
        });
    });
    if (changed == invalidations.end()) {
        transaction.stage = HotReloadStage::Detect;
        return transaction;
    }
    const auto after_entry = std::ranges::find_if(after.entries, [&](const AssetRegistryEntry& entry) {
        return entry.id.ToString() == asset_id.ToString();
    });
    if (after_entry == after.entries.end() || !after_entry->diagnostics.empty()) {
        transaction.stage = HotReloadStage::RolledBack;
        transaction.rollback_required = true;
        for (const auto& diagnostic : after_entry == after.entries.end() ? std::vector<Astra::Core::Diagnostic>{MakeDiagnostic("ASTRA_HOT_RELOAD_ASSET_REMOVED", Astra::Core::DiagnosticSeverity::Warning, "Hot reload asset was removed.")} : after_entry->diagnostics) {
            transaction.diagnostics.push_back(diagnostic);
            diagnostics.Emit(diagnostic);
        }
        return transaction;
    }
    transaction.stage = HotReloadStage::SwitchAtFrameBoundary;
    transaction.old_resource_retained = false;
    return transaction;
}

} // namespace Astra::Asset
