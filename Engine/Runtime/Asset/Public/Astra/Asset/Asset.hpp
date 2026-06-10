#pragma once

#include <Astra/Core/Diagnostics.hpp>
#include <Astra/Core/StableId.hpp>
#include <Astra/Core/Types.hpp>
#include <Astra/Asset/Export.hpp>
#include <Astra/Platform/Platform.hpp>
#include <nlohmann/json.hpp>

#include <filesystem>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace Astra::Asset {

constexpr const char* AssetSidecarSchema = "astra.asset.sidecar.v1";
constexpr const char* AssetRegistrySchema = "astra.asset.registry.v1";
constexpr const char* CookManifestSchema = "astra.asset.cook_manifest.v1";
constexpr const char* DerivedDataCacheEntrySchema = "astra.asset.ddc_entry.v1";
constexpr const char* PackageManifestSchema = "astra.package.manifest.v1";

enum class AssetScheme {
    Native,
    Virtual,
    Foreign,
    Package,
    Asset
};

struct AssetUri {
    AssetScheme scheme = AssetScheme::Asset;
    std::string scheme_name = "asset";
    std::string path;
    std::string fragment;

    [[nodiscard]] ASTRA_ASSET_API std::string ToString() const;
    [[nodiscard]] ASTRA_ASSET_API Astra::Core::AssetId ToStableId() const;
};

enum class VfsAccess {
    ReadOnly,
    ReadWrite
};

struct VfsMount {
    std::string mount_id;
    std::string scheme = "native";
    std::filesystem::path root;
    VfsAccess access = VfsAccess::ReadOnly;
    Astra::Core::i32 priority = 0;
    bool allow_cook = true;
    bool allow_package = true;
};

struct ResolvedAssetPath {
    AssetUri uri;
    VfsMount mount;
    std::filesystem::path path;
    bool read_only = true;
};

struct AssetLicense {
    std::string owner;
    std::string usage;
    std::string source;
};

struct AssetReview {
    std::string status = "accepted";
    std::string review_item;
};

struct AssetCookSettings {
    std::string preset;
    nlohmann::json settings = nlohmann::json::object();
};

struct AiGenerationInfo {
    std::string provider;
    std::string session_hash;
    std::string prompt_hash;
    std::string context_hash;
    std::string output_hash;
    std::string source_draft;
};

struct AssetSidecar {
    std::string schema = AssetSidecarSchema;
    AssetUri id;
    std::string type;
    std::filesystem::path source_path;
    std::string display_name;
    std::vector<std::string> tags;
    std::string origin = "HumanAuthored";
    AssetLicense license;
    AssetReview review;
    AssetCookSettings cook;
    std::vector<AssetUri> hard_dependencies;
    std::vector<AssetUri> soft_dependencies;
    std::optional<AiGenerationInfo> ai_generation;
    bool requires_review = false;
};

struct AssetRegistryEntry {
    AssetUri id;
    std::string type;
    std::filesystem::path sidecar_path;
    std::filesystem::path source_path;
    std::string source_hash;
    std::string sidecar_hash;
    std::vector<AssetUri> hard_dependencies;
    std::vector<AssetUri> soft_dependencies;
    std::vector<Astra::Core::Diagnostic> diagnostics;
};

struct AssetRegistry {
    std::string schema = AssetRegistrySchema;
    Astra::Core::u32 version = 1;
    std::vector<AssetRegistryEntry> entries;
};

struct ImportPresetDescriptor {
    std::string id;
    std::vector<std::string> source_extensions;
    std::string asset_type;
    nlohmann::json sidecar_defaults = nlohmann::json::object();
    nlohmann::json cook_defaults = nlohmann::json::object();
    nlohmann::json license_policy = nlohmann::json::object();
};

struct ProjectTemplateDescriptor {
    std::string id;
    std::string display_name;
    std::string runtime_profile;
    std::map<std::string, std::string> engine_modules;
    std::vector<std::string> required_fields;
    std::vector<std::string> acceptance_commands;
};

struct ReviewQueueItem {
    std::string id;
    std::string kind;
    std::string state = "pending";
    std::string source_ref;
    std::string target_ref;
};

struct AssetInvalidation {
    std::filesystem::path path;
    std::vector<AssetUri> affected_assets;
    std::string reason;
};

struct AssetDependencyGraph {
    std::string schema = "astra.asset.dependency_graph.v1";
    std::vector<AssetRegistryEntry> assets;
    std::map<std::string, std::vector<std::string>> hard_edges;
    std::map<std::string, std::vector<std::string>> soft_edges;
};

struct DerivedDataCacheArtifact {
    std::filesystem::path path;
    std::string hash;
    Astra::Core::u64 size_bytes = 0;
};

struct DerivedDataCacheEntry {
    std::string schema = DerivedDataCacheEntrySchema;
    std::string key;
    std::string processor_id;
    std::string input_hash;
    std::string settings_hash;
    std::string output_hash;
    std::string platform = "win64";
    std::string profile = "development";
    std::string engine_version;
    bool rebuilt = false;
    bool corruption_recovered = false;
    std::vector<DerivedDataCacheArtifact> artifacts;
};

struct CookArtifact {
    AssetUri asset_id;
    std::string type;
    std::filesystem::path artifact_path;
    std::string hash;
    std::string processor_id;
    std::string ddc_key;
    nlohmann::json metadata = nlohmann::json::object();
};

struct CookManifest {
    std::string schema = CookManifestSchema;
    std::string project_id;
    std::string profile = "development";
    std::string manifest_hash;
    std::vector<CookArtifact> artifacts;
    std::vector<DerivedDataCacheEntry> ddc_entries;
};

struct PackageModuleEvidence {
    std::string id;
    std::string binary;
    std::string sha256;
    bool runtime_safe = true;
};

struct PackagePayloadEntry {
    AssetUri asset_id;
    std::filesystem::path artifact_path;
    std::string hash;
    Astra::Core::u64 size_bytes = 0;
    std::string encoding = "base64";
    std::string data;
};

struct PackagePayloadChunk {
    AssetUri asset_id;
    Astra::Core::u64 offset = 0;
    std::vector<Astra::Core::u8> bytes;
    bool final_chunk = false;
};

struct PackageManifest {
    std::string schema = PackageManifestSchema;
    std::string package_id;
    std::string profile = "deterministic";
    std::string package_hash;
    std::string project_hash;
    CookManifest cook_manifest;
    std::vector<PackageModuleEvidence> modules;
    std::vector<PackagePayloadEntry> payloads;
    nlohmann::json runtime_evidence = nlohmann::json::object();
};

struct PackageMount {
    std::string schema = "astra.package.mount.v1";
    std::string package_id;
    std::filesystem::path package_path;
    bool read_only = true;
    std::vector<AssetUri> assets;
};

class PackageReader {
public:
    [[nodiscard]] ASTRA_ASSET_API Astra::Core::Result<PackageManifest> ReadManifest(const std::filesystem::path& package_path, Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] ASTRA_ASSET_API Astra::Core::Result<std::vector<Astra::Core::u8>> ReadPayloadBytes(const std::filesystem::path& package_path, const AssetUri& asset_id, Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] ASTRA_ASSET_API Astra::Core::Result<std::vector<PackagePayloadChunk>> ReadPayloadChunks(const std::filesystem::path& package_path, const AssetUri& asset_id, Astra::Core::u64 chunk_size, Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] ASTRA_ASSET_API Astra::Core::Result<std::string> ReadPayloadText(const std::filesystem::path& package_path, const AssetUri& asset_id, Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] ASTRA_ASSET_API Astra::Core::Result<PackageMount> MountPackage(const std::filesystem::path& package_path, Astra::Core::DiagnosticSink& diagnostics) const;
};

class Vfs {
public:
    [[nodiscard]] ASTRA_ASSET_API Astra::Core::Result<void> Mount(VfsMount mount, Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] ASTRA_ASSET_API std::optional<ResolvedAssetPath> Resolve(const AssetUri& uri) const;
    [[nodiscard]] ASTRA_ASSET_API const std::vector<VfsMount>& Mounts() const;

private:
    std::vector<VfsMount> mounts_;
};

class AssetRegistryBuilder {
public:
    [[nodiscard]] ASTRA_ASSET_API AssetRegistry Scan(const std::filesystem::path& content_root, Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] ASTRA_ASSET_API std::vector<AssetInvalidation> WatchContent(Astra::Platform::IFileSystemService& filesystem, const std::filesystem::path& content_root) const;
};

[[nodiscard]] ASTRA_ASSET_API Astra::Core::Result<AssetUri> ParseAssetUri(std::string_view text);
[[nodiscard]] ASTRA_ASSET_API std::string ToString(AssetScheme scheme);
[[nodiscard]] ASTRA_ASSET_API AssetScheme AssetSchemeFromString(std::string_view value);
[[nodiscard]] ASTRA_ASSET_API Astra::Core::Result<AssetSidecar> AssetSidecarFromJson(const nlohmann::json& json, const std::filesystem::path& sidecar_path, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_ASSET_API Astra::Core::Result<AssetSidecar> LoadAssetSidecar(const std::filesystem::path& path, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_ASSET_API Astra::Core::Result<void> ValidateAssetSidecar(const AssetSidecar& sidecar, const std::filesystem::path& sidecar_path, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_ASSET_API Astra::Core::Result<void> ValidateImportPreset(const ImportPresetDescriptor& descriptor, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_ASSET_API Astra::Core::Result<void> ValidateProjectTemplate(const ProjectTemplateDescriptor& descriptor, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] ASTRA_ASSET_API Astra::Core::Result<void> ValidateReviewQueueItem(const ReviewQueueItem& item, Astra::Core::DiagnosticSink& diagnostics);

[[nodiscard]] ASTRA_ASSET_API nlohmann::json ToJson(const AssetUri& uri);
[[nodiscard]] ASTRA_ASSET_API nlohmann::json ToJson(const AssetSidecar& sidecar);
[[nodiscard]] ASTRA_ASSET_API nlohmann::json ToJson(const AssetRegistryEntry& entry);
[[nodiscard]] ASTRA_ASSET_API nlohmann::json ToJson(const AssetRegistry& registry);
[[nodiscard]] ASTRA_ASSET_API nlohmann::json ToJson(const AssetDependencyGraph& graph);
[[nodiscard]] ASTRA_ASSET_API nlohmann::json ToJson(const DerivedDataCacheArtifact& artifact);
[[nodiscard]] ASTRA_ASSET_API nlohmann::json ToJson(const DerivedDataCacheEntry& entry);
[[nodiscard]] ASTRA_ASSET_API nlohmann::json ToJson(const CookArtifact& artifact);
[[nodiscard]] ASTRA_ASSET_API nlohmann::json ToJson(const CookManifest& manifest);
[[nodiscard]] ASTRA_ASSET_API nlohmann::json ToJson(const PackageModuleEvidence& module);
[[nodiscard]] ASTRA_ASSET_API nlohmann::json ToJson(const PackagePayloadEntry& payload);
[[nodiscard]] ASTRA_ASSET_API nlohmann::json ToJson(const PackageManifest& manifest);
[[nodiscard]] ASTRA_ASSET_API nlohmann::json ToJson(const PackageMount& mount);
[[nodiscard]] ASTRA_ASSET_API AssetDependencyGraph BuildDependencyGraph(const AssetRegistry& registry);
[[nodiscard]] ASTRA_ASSET_API Astra::Core::Result<PackageManifest> PackageManifestFromJson(const nlohmann::json& json);
[[nodiscard]] ASTRA_ASSET_API std::string ComputeCookManifestHash(nlohmann::json manifest_json);
[[nodiscard]] ASTRA_ASSET_API std::string ComputePackageManifestHash(nlohmann::json manifest_json);

} // namespace Astra::Asset
