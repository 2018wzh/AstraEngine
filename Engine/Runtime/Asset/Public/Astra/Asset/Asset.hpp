#pragma once

#include <Astra/Core/Diagnostics.hpp>
#include <Astra/Core/StableId.hpp>
#include <Astra/Core/Types.hpp>
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

    [[nodiscard]] std::string ToString() const;
    [[nodiscard]] Astra::Core::AssetId ToStableId() const;
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

class Vfs {
public:
    [[nodiscard]] Astra::Core::Result<void> Mount(VfsMount mount, Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] std::optional<ResolvedAssetPath> Resolve(const AssetUri& uri) const;
    [[nodiscard]] const std::vector<VfsMount>& Mounts() const;

private:
    std::vector<VfsMount> mounts_;
};

class AssetRegistryBuilder {
public:
    [[nodiscard]] AssetRegistry Scan(const std::filesystem::path& content_root, Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] std::vector<AssetInvalidation> WatchContent(Astra::Platform::IFileSystemService& filesystem, const std::filesystem::path& content_root) const;
};

[[nodiscard]] Astra::Core::Result<AssetUri> ParseAssetUri(std::string_view text);
[[nodiscard]] std::string ToString(AssetScheme scheme);
[[nodiscard]] AssetScheme AssetSchemeFromString(std::string_view value);
[[nodiscard]] Astra::Core::Result<AssetSidecar> AssetSidecarFromJson(const nlohmann::json& json, const std::filesystem::path& sidecar_path, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] Astra::Core::Result<AssetSidecar> LoadAssetSidecar(const std::filesystem::path& path, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] Astra::Core::Result<void> ValidateAssetSidecar(const AssetSidecar& sidecar, const std::filesystem::path& sidecar_path, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] Astra::Core::Result<void> ValidateImportPreset(const ImportPresetDescriptor& descriptor, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] Astra::Core::Result<void> ValidateProjectTemplate(const ProjectTemplateDescriptor& descriptor, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] Astra::Core::Result<void> ValidateReviewQueueItem(const ReviewQueueItem& item, Astra::Core::DiagnosticSink& diagnostics);

[[nodiscard]] nlohmann::json ToJson(const AssetUri& uri);
[[nodiscard]] nlohmann::json ToJson(const AssetSidecar& sidecar);
[[nodiscard]] nlohmann::json ToJson(const AssetRegistryEntry& entry);
[[nodiscard]] nlohmann::json ToJson(const AssetRegistry& registry);

} // namespace Astra::Asset
