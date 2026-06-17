#include <Astra/Asset/Asset.hpp>

#include "AssetInternal.hpp"

#include <Astra/Core/Logging.hpp>

#include <algorithm>
#include <array>
#include <cctype>
#include <fstream>
#include <set>

namespace Astra::Asset {

namespace {

bool JsonValueAsBool(const nlohmann::json& json, std::string_view key, bool fallback) {
    if (!json.contains(key)) {
        return fallback;
    }
    const auto& value = json.at(key);
    if (value.is_boolean()) {
        return value.get<bool>();
    }
    if (value.is_string()) {
        auto text = value.get<std::string>();
        std::ranges::transform(text, text.begin(), [](unsigned char character) {
            return static_cast<char>(std::tolower(character));
        });
        if (text == "true" || text == "1") {
            return true;
        }
        if (text == "false" || text == "0") {
            return false;
        }
    }
    if (value.is_number_integer()) {
        return value.get<int>() != 0;
    }
    return fallback;
}

template <typename T>
void WritePod(std::ofstream& file, T value) {
    file.write(reinterpret_cast<const char*>(&value), sizeof(T));
}

constexpr std::array<char, 8> kPackageMagic = {'A', 'S', 'T', 'R', 'A', 'P', '6', '\0'};
constexpr Astra::Core::u32 kPackageVersion = 1;

Astra::Core::u64 BinaryPackageHeaderSize() {
    return static_cast<Astra::Core::u64>(kPackageMagic.size() + sizeof(Astra::Core::u32) + sizeof(Astra::Core::u64));
}

} // namespace

nlohmann::json ToJson(const AssetUri& uri) {
    return uri.ToString();
}

nlohmann::json ToJson(const AssetSidecar& sidecar) {
    nlohmann::json hard = nlohmann::json::array();
    for (const auto& dependency : sidecar.hard_dependencies) {
        hard.push_back(dependency.ToString());
    }
    nlohmann::json soft = nlohmann::json::array();
    for (const auto& dependency : sidecar.soft_dependencies) {
        soft.push_back(dependency.ToString());
    }
    return {
        {"schema", sidecar.schema},
        {"id", sidecar.id.ToString()},
        {"type", sidecar.type},
        {"source_path", sidecar.source_path.string()},
        {"display_name", sidecar.display_name},
        {"tags", sidecar.tags},
        {"origin", sidecar.origin},
        {"license", {{"owner", sidecar.license.owner}, {"usage", sidecar.license.usage}, {"source", sidecar.license.source}}},
        {"review", {{"status", sidecar.review.status}, {"review_item", sidecar.review.review_item}}},
        {"cook", {{"preset", sidecar.cook.preset}, {"settings", sidecar.cook.settings}}},
        {"dependencies", {{"hard", hard}, {"soft", soft}}},
        {"requires_review", sidecar.requires_review},
    };
}

nlohmann::json ToJson(const AssetRegistryEntry& entry) {
    nlohmann::json hard = nlohmann::json::array();
    for (const auto& dependency : entry.hard_dependencies) {
        hard.push_back(dependency.ToString());
    }
    nlohmann::json soft = nlohmann::json::array();
    for (const auto& dependency : entry.soft_dependencies) {
        soft.push_back(dependency.ToString());
    }
    nlohmann::json diagnostics = nlohmann::json::array();
    for (const auto& diagnostic : entry.diagnostics) {
        diagnostics.push_back(Astra::Core::ToJson(diagnostic));
    }
    return {
        {"id", entry.id.ToString()},
        {"type", entry.type},
        {"sidecar_path", entry.sidecar_path.string()},
        {"source_path", entry.source_path.string()},
        {"source_hash", entry.source_hash},
        {"sidecar_hash", entry.sidecar_hash},
        {"cook_key", entry.cook_key},
        {"importer_id", entry.importer_id},
        {"origin", entry.origin},
        {"license", {{"owner", entry.license.owner}, {"usage", entry.license.usage}, {"source", entry.license.source}}},
        {"review", {{"status", entry.review.status}, {"review_item", entry.review.review_item}}},
        {"requires_review", entry.requires_review},
        {"has_ai_generation", entry.has_ai_generation},
        {"dependencies", {{"hard", hard}, {"soft", soft}}},
        {"diagnostics", diagnostics},
    };
}

nlohmann::json ToJson(const AssetRegistry& registry) {
    nlohmann::json entries = nlohmann::json::array();
    for (const auto& entry : registry.entries) {
        entries.push_back(ToJson(entry));
    }
    return {{"schema", registry.schema}, {"version", registry.version}, {"entries", entries}};
}

AssetDependencyGraph BuildDependencyGraph(const AssetRegistry& registry) {
    AssetDependencyGraph graph;
    graph.assets = registry.entries;
    for (const auto& entry : registry.entries) {
        auto& hard = graph.hard_edges[entry.id.ToString()];
        for (const auto& dependency : entry.hard_dependencies) {
            hard.push_back(dependency.ToString());
        }
        auto& soft = graph.soft_edges[entry.id.ToString()];
        for (const auto& dependency : entry.soft_dependencies) {
            soft.push_back(dependency.ToString());
        }
    }
    return graph;
}

nlohmann::json ToJson(const AssetDependencyGraph& graph) {
    nlohmann::json assets = nlohmann::json::array();
    for (const auto& asset : graph.assets) {
        assets.push_back(ToJson(asset));
    }
    return {{"schema", graph.schema}, {"assets", assets}, {"hard_edges", graph.hard_edges}, {"soft_edges", graph.soft_edges}};
}

nlohmann::json ToJson(const ImportRequest& request) {
    return {
        {"schema", request.schema},
        {"source_path", request.source_path.string()},
        {"target_asset_id", request.target_asset_id.ToString()},
        {"asset_type", request.asset_type},
        {"preset", request.preset},
        {"origin", request.origin},
        {"review_state", request.review_state},
        {"license", {{"owner", request.license.owner}, {"usage", request.license.usage}, {"source", request.license.source}}},
        {"copy_source", request.copy_source},
        {"foreign_copy_allowed", request.foreign_copy_allowed},
    };
}

nlohmann::json ToJson(const ImporterDescriptor& descriptor) {
    return {
        {"provider_id", descriptor.provider_id},
        {"contract", descriptor.contract},
        {"source_extensions", descriptor.source_extensions},
        {"output_asset_types", descriptor.output_asset_types},
        {"sidecar_schema", descriptor.sidecar_schema},
        {"diagnostics_prefix", descriptor.diagnostics_prefix},
        {"capabilities", {{"preview_metadata", descriptor.preview_metadata}, {"batch_import", descriptor.batch_import}, {"ai_draft_import", descriptor.ai_draft_import}}},
        {"permissions", {{"project_write", descriptor.project_write}, {"foreign_read", descriptor.foreign_read}}},
    };
}

nlohmann::json ToJson(const DdcKey& key) {
    return {
        {"schema", key.schema},
        {"asset_id", key.asset_id.ToString()},
        {"source_hash", key.source_hash},
        {"sidecar_hash", key.sidecar_hash},
        {"processor_id", key.processor_id},
        {"processor_version", key.processor_version},
        {"platform", key.platform},
        {"profile", key.profile},
        {"provider_feature_hash", key.provider_feature_hash},
        {"key", key.ToString()},
    };
}

nlohmann::json ToJson(const CookArtifactDescriptor& descriptor) {
    nlohmann::json dependencies = nlohmann::json::array();
    for (const auto& dependency : descriptor.runtime_dependencies) {
        dependencies.push_back(dependency.ToString());
    }
    return {
        {"schema", descriptor.schema},
        {"artifact_id", descriptor.artifact_id},
        {"asset_id", descriptor.asset_id.ToString()},
        {"format", descriptor.format},
        {"ddc_key", descriptor.ddc_key},
        {"payload_hash", descriptor.payload_hash},
        {"payload_size", descriptor.payload_size},
        {"runtime_dependencies", dependencies},
        {"metadata", descriptor.metadata},
    };
}

nlohmann::json ToJson(const CookProcessorDescriptor& descriptor) {
    return {
        {"provider_id", descriptor.provider_id},
        {"contract", descriptor.contract},
        {"input_asset_types", descriptor.input_asset_types},
        {"output_formats", descriptor.output_formats},
        {"requires_providers", descriptor.requires_providers},
        {"package_eligible", descriptor.package_eligible},
        {"version", descriptor.version},
        {"diagnostics_prefix", descriptor.diagnostics_prefix},
    };
}

nlohmann::json ToJson(const DerivedDataCacheArtifact& artifact) {
    return {
        {"path", artifact.path.string()},
        {"hash", artifact.hash},
        {"size_bytes", artifact.size_bytes},
    };
}

nlohmann::json ToJson(const DerivedDataCacheEntry& entry) {
    nlohmann::json artifacts = nlohmann::json::array();
    for (const auto& artifact : entry.artifacts) {
        artifacts.push_back(ToJson(artifact));
    }
    return {
        {"schema", entry.schema},
        {"key", entry.key},
        {"processor_id", entry.processor_id},
        {"input_hash", entry.input_hash},
        {"settings_hash", entry.settings_hash},
        {"output_hash", entry.output_hash},
        {"platform", entry.platform},
        {"profile", entry.profile},
        {"engine_version", entry.engine_version},
        {"rebuilt", entry.rebuilt},
        {"corruption_recovered", entry.corruption_recovered},
        {"artifacts", artifacts},
    };
}

nlohmann::json ToJson(const DdcCleanReport& report) {
    nlohmann::json diagnostics = nlohmann::json::array();
    for (const auto& diagnostic : report.diagnostics) {
        diagnostics.push_back(Astra::Core::ToJson(diagnostic));
    }
    return {
        {"schema", report.schema},
        {"root", report.root.string()},
        {"scanned", report.scanned},
        {"retained", report.retained},
        {"removed", report.removed},
        {"diagnostics", diagnostics},
    };
}

nlohmann::json ToJson(const CookArtifact& artifact) {
    return {
        {"asset_id", artifact.asset_id.ToString()},
        {"type", artifact.type},
        {"artifact_path", artifact.artifact_path.string()},
        {"hash", artifact.hash},
        {"processor_id", artifact.processor_id},
        {"ddc_key", artifact.ddc_key},
        {"metadata", artifact.metadata},
    };
}

nlohmann::json ToJson(const CookManifest& manifest) {
    nlohmann::json artifacts = nlohmann::json::array();
    for (const auto& artifact : manifest.artifacts) {
        artifacts.push_back(ToJson(artifact));
    }
    nlohmann::json ddc_entries = nlohmann::json::array();
    for (const auto& entry : manifest.ddc_entries) {
        ddc_entries.push_back(ToJson(entry));
    }
    return {
        {"schema", manifest.schema},
        {"project_id", manifest.project_id},
        {"profile", manifest.profile},
        {"manifest_hash", manifest.manifest_hash},
        {"artifacts", artifacts},
        {"ddc_entries", ddc_entries},
    };
}

nlohmann::json ToJson(const PackageModuleEvidence& module) {
    return {
        {"id", module.id},
        {"binary", module.binary},
        {"sha256", module.sha256},
        {"runtime_safe", module.runtime_safe},
    };
}

nlohmann::json ToJson(const PackagePayloadEntry& payload) {
    return {
        {"asset_id", payload.asset_id.ToString()},
        {"artifact_path", payload.artifact_path.string()},
        {"hash", payload.hash},
        {"size_bytes", payload.size_bytes},
        {"encoding", payload.encoding},
        {"data", payload.data},
        {"offset", payload.offset},
        {"compressed_size", payload.compressed_size},
        {"compression", payload.compression},
        {"streaming", payload.streaming},
    };
}

nlohmann::json ToJson(const PackagePayloadRef& payload) {
    return {
        {"schema", payload.schema},
        {"asset_id", payload.asset_id.ToString()},
        {"artifact_id", payload.artifact_id},
        {"offset", payload.offset},
        {"size", payload.size},
        {"compressed_size", payload.compressed_size},
        {"hash", payload.hash},
        {"compression", payload.compression},
        {"streaming", payload.streaming},
    };
}

nlohmann::json ToJson(const PackageManifest& manifest) {
    nlohmann::json modules = nlohmann::json::array();
    for (const auto& module : manifest.modules) {
        modules.push_back(ToJson(module));
    }
    nlohmann::json payloads = nlohmann::json::array();
    for (const auto& payload : manifest.payloads) {
        payloads.push_back(ToJson(payload));
    }
    return {
        {"schema", manifest.schema},
        {"package_id", manifest.package_id},
        {"profile", manifest.profile},
        {"package_hash", manifest.package_hash},
        {"project_hash", manifest.project_hash},
        {"cook_manifest", ToJson(manifest.cook_manifest)},
        {"modules", modules},
        {"payloads", payloads},
        {"runtime_evidence", manifest.runtime_evidence},
    };
}

nlohmann::json ToJson(const PackageMountPolicy& policy) {
    return {
        {"schema", policy.schema},
        {"mount", policy.mount},
        {"read_only", policy.read_only},
        {"allow_random_access", policy.allow_random_access},
        {"allow_chunked_read", policy.allow_chunked_read},
        {"foreign_copy_allowed", policy.foreign_copy_allowed},
    };
}

nlohmann::json ToJson(const PackageMount& mount) {
    nlohmann::json assets = nlohmann::json::array();
    for (const auto& asset : mount.assets) {
        assets.push_back(asset.ToString());
    }
    return {
        {"schema", mount.schema},
        {"package_id", mount.package_id},
        {"package_path", mount.package_path.string()},
        {"read_only", mount.read_only},
        {"policy", ToJson(mount.policy)},
        {"assets", assets},
    };
}

nlohmann::json ToJson(const AssetReleaseGateReport& report) {
    nlohmann::json diagnostics = nlohmann::json::array();
    for (const auto& diagnostic : report.diagnostics) {
        diagnostics.push_back(Astra::Core::ToJson(diagnostic));
    }
    return {
        {"schema", report.schema},
        {"passed", report.passed},
        {"diagnostics", diagnostics},
        {"evidence", report.evidence},
    };
}

std::string ToString(HotReloadStage stage) {
    switch (stage) {
    case HotReloadStage::Detect:
        return "detect";
    case HotReloadStage::Validate:
        return "validate";
    case HotReloadStage::CookTemp:
        return "cook_temp";
    case HotReloadStage::PrepareProviderResource:
        return "prepare_provider_resource";
    case HotReloadStage::SwitchAtFrameBoundary:
        return "switch_at_frame_boundary";
    case HotReloadStage::RetireOldResource:
        return "retire_old_resource";
    case HotReloadStage::RolledBack:
        return "rolled_back";
    }
    return "detect";
}

nlohmann::json ToJson(const HotReloadTransaction& transaction) {
    nlohmann::json diagnostics = nlohmann::json::array();
    for (const auto& diagnostic : transaction.diagnostics) {
        diagnostics.push_back(Astra::Core::ToJson(diagnostic));
    }
    return {
        {"schema", transaction.schema},
        {"transaction_id", transaction.transaction_id},
        {"asset_id", transaction.asset_id.ToString()},
        {"stage", ToString(transaction.stage)},
        {"rollback_required", transaction.rollback_required},
        {"old_resource_retained", transaction.old_resource_retained},
        {"provider_id", transaction.provider_id},
        {"diagnostics", diagnostics},
    };
}

Astra::Core::Result<PackageManifest> PackageManifestFromJson(const nlohmann::json& json) {
    if (json.value("schema", std::string()) != PackageManifestSchema) {
        return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "unsupported package manifest schema");
    }
    PackageManifest manifest;
    manifest.package_id = json.value("package_id", "");
    manifest.profile = json.value("profile", "deterministic");
    manifest.package_hash = json.value("package_hash", "");
    manifest.project_hash = json.value("project_hash", "");
    manifest.cook_manifest = {};
    if (json.contains("cook_manifest")) {
        manifest.cook_manifest.schema = json.at("cook_manifest").value("schema", CookManifestSchema);
        manifest.cook_manifest.project_id = json.at("cook_manifest").value("project_id", "");
        manifest.cook_manifest.profile = json.at("cook_manifest").value("profile", "development");
        manifest.cook_manifest.manifest_hash = json.at("cook_manifest").value("manifest_hash", "");
        for (const auto& artifact_json : json.at("cook_manifest").value("artifacts", nlohmann::json::array())) {
            CookArtifact artifact;
            auto asset_id = ParseAssetUri(artifact_json.value("asset_id", "asset:/invalid"));
            if (!asset_id) {
                return Astra::Core::Result<PackageManifest>::Failure(asset_id.Error(), asset_id.Message());
            }
            artifact.asset_id = asset_id.Value();
            artifact.type = artifact_json.value("type", "");
            artifact.artifact_path = artifact_json.value("artifact_path", "");
            artifact.hash = artifact_json.value("hash", "");
            artifact.processor_id = artifact_json.value("processor_id", "");
            artifact.ddc_key = artifact_json.value("ddc_key", "");
            artifact.metadata = artifact_json.value("metadata", nlohmann::json::object());
            manifest.cook_manifest.artifacts.push_back(std::move(artifact));
        }
        for (const auto& entry_json : json.at("cook_manifest").value("ddc_entries", nlohmann::json::array())) {
            DerivedDataCacheEntry entry;
            entry.key = entry_json.value("key", "");
            entry.processor_id = entry_json.value("processor_id", "");
            entry.input_hash = entry_json.value("input_hash", "");
            entry.settings_hash = entry_json.value("settings_hash", "");
            entry.output_hash = entry_json.value("output_hash", "");
            entry.platform = entry_json.value("platform", "win64");
            entry.profile = entry_json.value("profile", "development");
            entry.engine_version = entry_json.value("engine_version", "");
            entry.rebuilt = JsonValueAsBool(entry_json, "rebuilt", false);
            entry.corruption_recovered = JsonValueAsBool(entry_json, "corruption_recovered", false);
            for (const auto& artifact_json : entry_json.value("artifacts", nlohmann::json::array())) {
                DerivedDataCacheArtifact artifact;
                artifact.path = artifact_json.value("path", "");
                artifact.hash = artifact_json.value("hash", "");
                artifact.size_bytes = artifact_json.value("size_bytes", 0ull);
                entry.artifacts.push_back(std::move(artifact));
            }
            manifest.cook_manifest.ddc_entries.push_back(std::move(entry));
        }
    }
    for (const auto& module_json : json.value("modules", nlohmann::json::array())) {
        PackageModuleEvidence module;
        module.id = module_json.value("id", "");
        module.binary = module_json.value("binary", "");
        module.sha256 = module_json.value("sha256", "");
        module.runtime_safe = JsonValueAsBool(module_json, "runtime_safe", true);
        manifest.modules.push_back(std::move(module));
    }
    for (const auto& payload_json : json.value("payloads", nlohmann::json::array())) {
        auto asset_id = ParseAssetUri(payload_json.value("asset_id", "asset:/invalid"));
        if (!asset_id) {
            return Astra::Core::Result<PackageManifest>::Failure(asset_id.Error(), asset_id.Message());
        }
        PackagePayloadEntry payload;
        payload.asset_id = asset_id.Value();
        payload.artifact_path = payload_json.value("artifact_path", "");
        payload.hash = payload_json.value("hash", "");
        payload.size_bytes = payload_json.value("size_bytes", 0ull);
        payload.encoding = payload_json.value("encoding", "base64");
        payload.data = payload_json.value("data", "");
        payload.offset = payload_json.value("offset", 0ull);
        payload.compressed_size = payload_json.value("compressed_size", 0ull);
        payload.compression = payload_json.value("compression", "none");
        payload.streaming = payload_json.value("streaming", "inline");
        manifest.payloads.push_back(std::move(payload));
    }
    manifest.runtime_evidence = json.value("runtime_evidence", nlohmann::json::object());
    return Astra::Core::Result<PackageManifest>::Success(std::move(manifest));
}

std::string ComputeCookManifestHash(nlohmann::json manifest_json) {
    manifest_json.erase("manifest_hash");
    return Sha256Text(manifest_json.dump());
}

std::string ComputePackageManifestHash(nlohmann::json manifest_json) {
    manifest_json.erase("package_hash");
    return Sha256Text(manifest_json.dump());
}

Astra::Core::Result<PackageManifest> PackageReader::ReadManifest(const std::filesystem::path& package_path, Astra::Core::DiagnosticSink& diagnostics) const {
    Astra::Core::DefaultLogger().Log(
        "asset.package",
        "package_reader",
        Astra::Core::LogLevel::Debug,
        "package manifest read started",
        {{"package", package_path.string()}});
    auto package_json = ReadPackageJson(package_path, diagnostics);
    if (!package_json) {
        return Astra::Core::Result<PackageManifest>::Failure(package_json.Error(), package_json.Message());
    }
    const auto& json = package_json.Value();
    auto manifest = PackageManifestFromJson(json);
    if (!manifest) {
        return manifest;
    }
    const auto actual_package_hash = ComputePackageManifestHash(json);
    if (manifest.Value().package_hash != actual_package_hash) {
        auto diagnostic = MakeDiagnostic("ASTRA_PACKAGE_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Package hash mismatch.", package_path);
        Astra::Core::LogDiagnostic(diagnostic, "asset.package", "package_reader");
        diagnostics.Emit(std::move(diagnostic));
        return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package hash mismatch");
    }
    if (json.contains("cook_manifest")) {
        const auto cook_json = json.at("cook_manifest");
        const auto actual_cook_hash = ComputeCookManifestHash(cook_json);
        if (cook_json.value("manifest_hash", std::string()) != actual_cook_hash) {
            auto diagnostic = MakeDiagnostic("ASTRA_COOK_MANIFEST_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Cook manifest hash mismatch.", package_path);
            Astra::Core::LogDiagnostic(diagnostic, "asset.package", "package_reader");
            diagnostics.Emit(std::move(diagnostic));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "cook manifest hash mismatch");
        }
    }
    Astra::Core::DefaultLogger().Log(
        "asset.package",
        manifest.Value().package_id,
        Astra::Core::LogLevel::Debug,
        "package manifest read finished",
        {{"package", package_path.string()},
         {"package_hash", manifest.Value().package_hash},
         {"payloads", std::to_string(manifest.Value().payloads.size())}});
    return manifest;
}

Astra::Core::Result<std::vector<Astra::Core::u8>> PackageReader::ReadPayloadBytes(const std::filesystem::path& package_path, const AssetUri& asset_id, Astra::Core::DiagnosticSink& diagnostics) const {
    auto manifest = ReadManifest(package_path, diagnostics);
    if (!manifest) {
        return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(manifest.Error(), manifest.Message());
    }
    const auto& payloads = manifest.Value().payloads;
    const auto found = std::ranges::find_if(payloads, [&](const PackagePayloadEntry& payload) {
        return payload.asset_id.ToString() == asset_id.ToString();
    });
    if (found == payloads.end()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_NOT_FOUND", Astra::Core::DiagnosticSeverity::Blocking, "Package payload not found.", package_path));
        return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(Astra::Core::ErrorCode::NotFound, "package payload missing");
    }
    if (found->encoding == "base64") {
        auto decoded = DecodeBase64(found->data);
        if (!decoded) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_ENCODING", Astra::Core::DiagnosticSeverity::Blocking, decoded.Message(), package_path));
            return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(decoded.Error(), decoded.Message());
        }
        if (Sha256Bytes(decoded.Value()) != found->hash) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Package payload hash mismatch.", package_path));
            return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload hash mismatch");
        }
        return Astra::Core::Result<std::vector<Astra::Core::u8>>::Success(std::move(decoded.Value()));
    }
    if (found->encoding == "binary") {
        if (found->offset < BinaryPackageHeaderSize() || found->compressed_size == 0 || found->size_bytes == 0) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_REF_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Binary package payload ref is invalid.", package_path));
            return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload ref invalid");
        }
        std::ifstream package_file(package_path, std::ios::binary);
        package_file.seekg(static_cast<std::streamoff>(found->offset));
        std::vector<Astra::Core::u8> compressed(static_cast<std::size_t>(found->compressed_size));
        package_file.read(reinterpret_cast<char*>(compressed.data()), static_cast<std::streamsize>(compressed.size()));
        if (!package_file) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_TRUNCATED", Astra::Core::DiagnosticSeverity::Blocking, "Binary package payload is truncated.", package_path));
            return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload truncated");
        }
        if (found->compression == "zstd") {
            auto decoded = DecompressZstd(compressed, found->size_bytes);
            if (!decoded) {
                diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_COMPRESSION", Astra::Core::DiagnosticSeverity::Blocking, decoded.Message(), package_path));
                return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(decoded.Error(), decoded.Message());
            }
            if (Sha256Bytes(decoded.Value()) != found->hash) {
                diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Package payload hash mismatch.", package_path));
                return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload hash mismatch");
            }
            return decoded;
        }
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_ENCODING", Astra::Core::DiagnosticSeverity::Blocking, "Package payload encoding unsupported.", package_path));
        return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload encoding unsupported");
    }
    diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_ENCODING", Astra::Core::DiagnosticSeverity::Blocking, "Package payload encoding unsupported.", package_path));
    return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload encoding unsupported");
}

Astra::Core::Result<std::vector<PackagePayloadChunk>> PackageReader::ReadPayloadChunks(const std::filesystem::path& package_path, const AssetUri& asset_id, Astra::Core::u64 chunk_size, Astra::Core::DiagnosticSink& diagnostics) const {
    auto bytes = ReadPayloadBytes(package_path, asset_id, diagnostics);
    if (!bytes) {
        return Astra::Core::Result<std::vector<PackagePayloadChunk>>::Failure(bytes.Error(), bytes.Message());
    }
    std::vector<PackagePayloadChunk> chunks;
    const auto& payload = bytes.Value();
    for (std::size_t offset = 0; offset < payload.size(); offset += static_cast<std::size_t>(chunk_size)) {
        PackagePayloadChunk chunk;
        chunk.asset_id = asset_id;
        chunk.offset = static_cast<Astra::Core::u64>(offset);
        const auto end = std::min(payload.size(), offset + static_cast<std::size_t>(chunk_size));
        chunk.bytes.assign(payload.begin() + static_cast<std::ptrdiff_t>(offset), payload.begin() + static_cast<std::ptrdiff_t>(end));
        chunk.final_chunk = end >= payload.size();
        chunks.push_back(std::move(chunk));
    }
    return Astra::Core::Result<std::vector<PackagePayloadChunk>>::Success(std::move(chunks));
}

Astra::Core::Result<std::string> PackageReader::ReadPayloadText(const std::filesystem::path& package_path, const AssetUri& asset_id, Astra::Core::DiagnosticSink& diagnostics) const {
    auto bytes = ReadPayloadBytes(package_path, asset_id, diagnostics);
    if (!bytes) {
        return Astra::Core::Result<std::string>::Failure(bytes.Error(), bytes.Message());
    }
    return Astra::Core::Result<std::string>::Success(std::string(bytes.Value().begin(), bytes.Value().end()));
}

Astra::Core::Result<PackageMount> PackageReader::MountPackage(const std::filesystem::path& package_path, Astra::Core::DiagnosticSink& diagnostics) const {
    auto manifest = ReadManifest(package_path, diagnostics);
    if (!manifest) {
        return Astra::Core::Result<PackageMount>::Failure(manifest.Error(), manifest.Message());
    }
    PackageMount mount;
    mount.package_id = manifest.Value().package_id;
    mount.package_path = package_path;
    mount.assets.reserve(manifest.Value().payloads.size());
    for (const auto& payload : manifest.Value().payloads) {
        mount.assets.push_back(payload.asset_id);
    }
    return Astra::Core::Result<PackageMount>::Success(std::move(mount));
}

Astra::Core::Result<PackageManifest> PackageWriter::WritePackage(PackageManifest manifest, const std::filesystem::path& package_path, Astra::Core::DiagnosticSink& diagnostics) const {
    Astra::Core::DefaultLogger().Log(
        "asset.package",
        manifest.package_id,
        Astra::Core::LogLevel::Info,
        "package write started",
        {{"package", package_path.string()},
         {"profile", manifest.profile},
         {"artifacts", std::to_string(manifest.cook_manifest.artifacts.size())}});
    std::vector<std::vector<Astra::Core::u8>> compressed_payloads;
    compressed_payloads.reserve(manifest.cook_manifest.artifacts.size());
    manifest.payloads.clear();

    for (const auto& artifact : manifest.cook_manifest.artifacts) {
        if (!std::filesystem::exists(artifact.artifact_path)) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_SOURCE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Cook artifact payload is missing.", artifact.artifact_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::NotFound, "package payload source missing");
        }
        const auto bytes = ReadBytes(artifact.artifact_path);
        auto compressed = CompressZstd(bytes);
        if (compressed.empty() && !bytes.empty()) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_COMPRESSION", Astra::Core::DiagnosticSeverity::Blocking, "zstd compression failed for package payload.", artifact.artifact_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InternalError, "package payload compression failed");
        }
        PackagePayloadEntry payload;
        payload.asset_id = artifact.asset_id;
        payload.artifact_path = artifact.artifact_path;
        payload.hash = Sha256Bytes(bytes);
        payload.size_bytes = static_cast<Astra::Core::u64>(bytes.size());
        payload.encoding = "binary";
        payload.compressed_size = static_cast<Astra::Core::u64>(compressed.size());
        payload.compression = "zstd";
        payload.streaming = "chunked";
        manifest.payloads.push_back(std::move(payload));
        compressed_payloads.push_back(std::move(compressed));
    }

    if (manifest.profile.empty()) {
        manifest.profile = "deterministic";
    }
    if (manifest.cook_manifest.manifest_hash.empty()) {
        manifest.cook_manifest.manifest_hash = ComputeCookManifestHash(ToJson(manifest.cook_manifest));
    }
    manifest.package_hash = std::string(64, '0');

    nlohmann::json package_json;
    for (int pass = 0; pass < 3; ++pass) {
        package_json = ToJson(manifest);
        const auto manifest_text = package_json.dump();
        auto offset = BinaryPackageHeaderSize() + static_cast<Astra::Core::u64>(manifest_text.size());
        for (std::size_t index = 0; index < manifest.payloads.size(); ++index) {
            manifest.payloads[index].offset = offset;
            manifest.payloads[index].compressed_size = static_cast<Astra::Core::u64>(compressed_payloads[index].size());
            offset += manifest.payloads[index].compressed_size;
        }
        package_json = ToJson(manifest);
        package_json["package_hash"] = ComputePackageManifestHash(package_json);
        manifest.package_hash = package_json["package_hash"].get<std::string>();
    }

    const auto final_json = ToJson(manifest);
    const auto manifest_text = final_json.dump();
    std::filesystem::create_directories(package_path.parent_path());
    std::ofstream file(package_path, std::ios::binary);
    if (!file) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_WRITE_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "Package file could not be opened for writing.", package_path));
        return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::PermissionDenied, "package write failed");
    }
    file.write(kPackageMagic.data(), static_cast<std::streamsize>(kPackageMagic.size()));
    WritePod(file, kPackageVersion);
    WritePod(file, static_cast<Astra::Core::u64>(manifest_text.size()));
    file.write(manifest_text.data(), static_cast<std::streamsize>(manifest_text.size()));
    for (const auto& payload : compressed_payloads) {
        if (!payload.empty()) {
            file.write(reinterpret_cast<const char*>(payload.data()), static_cast<std::streamsize>(payload.size()));
        }
    }
    if (!file) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_WRITE_FAILED", Astra::Core::DiagnosticSeverity::Blocking, "Package file write failed.", package_path));
        return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InternalError, "package write failed");
    }
    Astra::Core::DefaultLogger().Log(
        "asset.package",
        manifest.package_id,
        Astra::Core::LogLevel::Info,
        "package write finished",
        {{"package", package_path.string()},
         {"package_hash", manifest.package_hash},
         {"payloads", std::to_string(manifest.payloads.size())}});
    return Astra::Core::Result<PackageManifest>::Success(std::move(manifest));
}

std::string ComputeAssetRegistryHash(const AssetRegistry& registry) {
    return Sha256Text(ToJson(registry).dump());
}

std::string ComputeProviderFeatureHash(const std::map<std::string, std::string>& selected_providers) {
    nlohmann::json providers = nlohmann::json::object();
    for (const auto& [slot, provider] : selected_providers) {
        providers[slot] = provider;
    }
    return Sha256Text(providers.dump());
}

} // namespace Astra::Asset
