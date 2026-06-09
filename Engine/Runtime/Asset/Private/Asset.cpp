#include <Astra/Asset/Asset.hpp>

#include <yaml-cpp/yaml.h>

#include <algorithm>
#include <fstream>
#include <iterator>
#include <set>
#include <sstream>

namespace Astra::Asset {

namespace {

std::string NormalizePath(std::string_view value) {
    std::string result(value);
    std::replace(result.begin(), result.end(), '\\', '/');
    while (result.find("//") != std::string::npos) {
        result.replace(result.find("//"), 2, "/");
    }
    if (!result.empty() && result.front() == '/') {
        result.erase(result.begin());
    }
    return result;
}

Astra::Core::Diagnostic MakeDiagnostic(std::string code, Astra::Core::DiagnosticSeverity severity, std::string message, const std::filesystem::path& path = {}) {
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = std::move(code);
    diagnostic.category = "asset.foundation";
    diagnostic.severity = severity;
    diagnostic.message = std::move(message);
    diagnostic.source.file = path.string();
    return diagnostic;
}

std::string ReadText(const std::filesystem::path& path) {
    std::ifstream file(path, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(file), {});
}

std::string StableHash(std::string_view text) {
    constexpr Astra::Core::u64 offset = 14695981039346656037ull;
    constexpr Astra::Core::u64 prime = 1099511628211ull;
    Astra::Core::u64 value = offset;
    for (const auto character : text) {
        value ^= static_cast<unsigned char>(character);
        value *= prime;
    }
    std::ostringstream output;
    output << std::hex << value;
    return output.str();
}

nlohmann::json YamlToJson(const YAML::Node& node) {
    if (!node) {
        return nullptr;
    }
    if (node.IsScalar()) {
        return node.as<std::string>();
    }
    if (node.IsSequence()) {
        nlohmann::json array = nlohmann::json::array();
        for (const auto& item : node) {
            array.push_back(YamlToJson(item));
        }
        return array;
    }
    if (node.IsMap()) {
        nlohmann::json object = nlohmann::json::object();
        for (const auto& item : node) {
            object[item.first.as<std::string>()] = YamlToJson(item.second);
        }
        return object;
    }
    return nullptr;
}

std::vector<AssetUri> AssetUriArray(const nlohmann::json& json, std::string_view key, Astra::Core::DiagnosticSink& diagnostics, const std::filesystem::path& sidecar_path) {
    std::vector<AssetUri> uris;
    if (!json.contains(key)) {
        return uris;
    }
    const auto& values = json.at(key);
    if (!values.is_array()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_DEPENDENCY_FORMAT", Astra::Core::DiagnosticSeverity::Blocking, "Asset dependency list must be an array.", sidecar_path));
        return uris;
    }
    for (const auto& value : values) {
        auto parsed = ParseAssetUri(value.get<std::string>());
        if (parsed) {
            uris.push_back(parsed.Value());
        } else {
            diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_DEPENDENCY_ID", Astra::Core::DiagnosticSeverity::Blocking, "Asset dependency has invalid id.", sidecar_path));
        }
    }
    return uris;
}

} // namespace

std::string AssetUri::ToString() const {
    std::string output = scheme_name + ":/" + NormalizePath(path);
    if (!fragment.empty()) {
        output += "#" + fragment;
    }
    return output;
}

Astra::Core::AssetId AssetUri::ToStableId() const {
    const auto parsed = Astra::Core::ParseStableId(ToString());
    if (parsed) {
        return parsed.Value();
    }
    return {};
}

std::string ToString(AssetScheme scheme) {
    switch (scheme) {
    case AssetScheme::Native:
        return "native";
    case AssetScheme::Virtual:
        return "virtual";
    case AssetScheme::Foreign:
        return "foreign";
    case AssetScheme::Package:
        return "package";
    case AssetScheme::Asset:
        return "asset";
    }
    return "asset";
}

AssetScheme AssetSchemeFromString(std::string_view value) {
    if (value == "native") {
        return AssetScheme::Native;
    }
    if (value == "virtual") {
        return AssetScheme::Virtual;
    }
    if (value == "package") {
        return AssetScheme::Package;
    }
    if (value.starts_with("foreign-")) {
        return AssetScheme::Foreign;
    }
    return AssetScheme::Asset;
}

Astra::Core::Result<AssetUri> ParseAssetUri(std::string_view text) {
    const auto split = text.find(":/");
    if (split == std::string_view::npos) {
        return Astra::Core::Result<AssetUri>::Failure(Astra::Core::ErrorCode::InvalidFormat, "asset uri must contain ':/'");
    }
    const auto scheme = text.substr(0, split);
    if (scheme != "asset" && scheme != "native" && scheme != "virtual" && scheme != "package" && !scheme.starts_with("foreign-")) {
        return Astra::Core::Result<AssetUri>::Failure(Astra::Core::ErrorCode::InvalidFormat, "asset uri has unsupported scheme");
    }
    auto rest = text.substr(split + 2);
    std::string fragment;
    const auto fragment_split = rest.find('#');
    if (fragment_split != std::string_view::npos) {
        fragment = std::string(rest.substr(fragment_split + 1));
        rest = rest.substr(0, fragment_split);
    }
    const auto normalized = NormalizePath(rest);
    if (normalized.empty() || normalized.find("..") != std::string::npos) {
        return Astra::Core::Result<AssetUri>::Failure(Astra::Core::ErrorCode::InvalidFormat, "asset uri path is empty or escapes its root");
    }
    return Astra::Core::Result<AssetUri>::Success({AssetSchemeFromString(scheme), std::string(scheme), normalized, fragment});
}

Astra::Core::Result<void> Vfs::Mount(VfsMount mount, Astra::Core::DiagnosticSink& diagnostics) {
    if (mount.mount_id.empty() || mount.scheme.empty() || mount.root.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_VFS_MOUNT_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "VFS mount requires mount id, scheme and root.", mount.root));
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidArgument, "invalid vfs mount");
    }
    mounts_.push_back(std::move(mount));
    std::ranges::sort(mounts_, [](const VfsMount& left, const VfsMount& right) {
        return left.priority > right.priority;
    });
    return Astra::Core::Result<void>::Success();
}

std::optional<ResolvedAssetPath> Vfs::Resolve(const AssetUri& uri) const {
    for (const auto& mount : mounts_) {
        if (mount.scheme == uri.scheme_name || (mount.scheme == "foreign" && uri.scheme == AssetScheme::Foreign)) {
            return ResolvedAssetPath{uri, mount, mount.root / uri.path, mount.access == VfsAccess::ReadOnly};
        }
    }
    return std::nullopt;
}

const std::vector<VfsMount>& Vfs::Mounts() const {
    return mounts_;
}

Astra::Core::Result<AssetSidecar> AssetSidecarFromJson(const nlohmann::json& json, const std::filesystem::path& sidecar_path, Astra::Core::DiagnosticSink& diagnostics) {
    if (!json.contains("id") || !json.contains("type") || !json.contains("source_path")) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_SIDECAR_REQUIRED", Astra::Core::DiagnosticSeverity::Blocking, "Asset sidecar requires id, type and source_path.", sidecar_path));
        return Astra::Core::Result<AssetSidecar>::Failure(Astra::Core::ErrorCode::InvalidFormat, "sidecar missing required fields");
    }
    auto id = ParseAssetUri(json.at("id").get<std::string>());
    if (!id) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_ID_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Asset sidecar id is invalid.", sidecar_path));
        return Astra::Core::Result<AssetSidecar>::Failure(id.Error(), id.Message());
    }
    AssetSidecar sidecar;
    sidecar.schema = json.value("schema", AssetSidecarSchema);
    sidecar.id = id.Value();
    sidecar.type = json.value("type", "");
    sidecar.source_path = json.value("source_path", "");
    sidecar.display_name = json.value("display_name", "");
    sidecar.tags = json.value("tags", std::vector<std::string>{});
    sidecar.origin = json.value("origin", "HumanAuthored");
    if (json.contains("license")) {
        sidecar.license.owner = json.at("license").value("owner", "");
        sidecar.license.usage = json.at("license").value("usage", "");
        sidecar.license.source = json.at("license").value("source", "");
    }
    if (json.contains("review")) {
        sidecar.review.status = json.at("review").value("status", "accepted");
        sidecar.review.review_item = json.at("review").value("review_item", "");
    }
    if (json.contains("cook")) {
        sidecar.cook.preset = json.at("cook").value("preset", json.at("cook").value("texture_preset", ""));
        sidecar.cook.settings = json.at("cook");
    }
    if (json.contains("dependencies")) {
        sidecar.hard_dependencies = AssetUriArray(json.at("dependencies"), "hard", diagnostics, sidecar_path);
        sidecar.soft_dependencies = AssetUriArray(json.at("dependencies"), "soft", diagnostics, sidecar_path);
    }
    sidecar.requires_review = json.value("requires_review", false);
    if (json.contains("ai_generation")) {
        AiGenerationInfo info;
        info.provider = json.at("ai_generation").value("provider", "");
        info.session_hash = json.at("ai_generation").value("session_hash", "");
        info.prompt_hash = json.at("ai_generation").value("prompt_hash", "");
        info.context_hash = json.at("ai_generation").value("context_hash", "");
        info.output_hash = json.at("ai_generation").value("output_hash", "");
        info.source_draft = json.at("ai_generation").value("source_draft", "");
        sidecar.ai_generation = std::move(info);
    }
    return Astra::Core::Result<AssetSidecar>::Success(std::move(sidecar));
}

Astra::Core::Result<AssetSidecar> LoadAssetSidecar(const std::filesystem::path& path, Astra::Core::DiagnosticSink& diagnostics) {
    try {
        const auto json = YamlToJson(YAML::LoadFile(path.string()));
        auto sidecar = AssetSidecarFromJson(json, path, diagnostics);
        if (!sidecar) {
            return sidecar;
        }
        auto validation = ValidateAssetSidecar(sidecar.Value(), path, diagnostics);
        if (!validation) {
            return Astra::Core::Result<AssetSidecar>::Failure(validation.Error(), validation.Message());
        }
        return sidecar;
    } catch (const YAML::Exception& error) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_SIDECAR_PARSE", Astra::Core::DiagnosticSeverity::Blocking, error.what(), path));
        return Astra::Core::Result<AssetSidecar>::Failure(Astra::Core::ErrorCode::InvalidFormat, error.what());
    }
}

Astra::Core::Result<void> ValidateAssetSidecar(const AssetSidecar& sidecar, const std::filesystem::path& sidecar_path, Astra::Core::DiagnosticSink& diagnostics) {
    bool valid = true;
    if (sidecar.id.scheme == AssetScheme::Virtual || sidecar.id.scheme == AssetScheme::Package) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_SIDECAR_SCHEME", Astra::Core::DiagnosticSeverity::Blocking, "Source sidecars must use native:/ or foreign-*:/ ids.", sidecar_path));
        valid = false;
    }
    if (sidecar.type.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_TYPE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Asset sidecar type is missing.", sidecar_path));
        valid = false;
    }
    if (sidecar.source_path.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_SOURCE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Asset sidecar source_path is missing.", sidecar_path));
        valid = false;
    }
    if (sidecar.license.owner.empty() || sidecar.license.usage.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_LICENSE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Asset sidecar license owner and usage are required.", sidecar_path));
        valid = false;
    }
    if ((sidecar.requires_review || sidecar.origin == "AIGenerated" || sidecar.ai_generation.has_value()) && sidecar.review.status != "accepted") {
        diagnostics.Emit(MakeDiagnostic("ASTRA_RELEASE_ASSET_004", Astra::Core::DiagnosticSeverity::Blocking, "Unreviewed AI asset cannot enter foundation registry.", sidecar_path));
        valid = false;
    }
    return valid ? Astra::Core::Result<void>::Success() : Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "asset sidecar validation failed");
}

AssetRegistry AssetRegistryBuilder::Scan(const std::filesystem::path& content_root, Astra::Core::DiagnosticSink& diagnostics) const {
    AssetRegistry registry;
    std::set<std::string> ids;
    if (!std::filesystem::exists(content_root)) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_CONTENT_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Content root does not exist.", content_root));
        return registry;
    }
    for (const auto& entry : std::filesystem::recursive_directory_iterator(content_root)) {
        if (!entry.is_regular_file() || entry.path().filename().string().find(".asset.yaml") == std::string::npos) {
            continue;
        }
        auto sidecar = LoadAssetSidecar(entry.path(), diagnostics);
        if (!sidecar) {
            continue;
        }
        AssetRegistryEntry registry_entry;
        registry_entry.id = sidecar.Value().id;
        registry_entry.type = sidecar.Value().type;
        registry_entry.sidecar_path = entry.path();
        registry_entry.source_path = entry.path().parent_path() / sidecar.Value().source_path;
        registry_entry.sidecar_hash = StableHash(ReadText(entry.path()));
        if (std::filesystem::exists(registry_entry.source_path)) {
            registry_entry.source_hash = StableHash(ReadText(registry_entry.source_path));
        } else {
            auto diagnostic = MakeDiagnostic("ASTRA_ASSET_SOURCE_NOT_FOUND", Astra::Core::DiagnosticSeverity::Blocking, "Asset source file is missing.", registry_entry.source_path);
            diagnostic.objects.push_back({"AssetId", registry_entry.id.ToString()});
            registry_entry.diagnostics.push_back(diagnostic);
            diagnostics.Emit(diagnostic);
        }
        if (!ids.insert(registry_entry.id.ToString()).second) {
            auto diagnostic = MakeDiagnostic("ASTRA_ASSET_ID_DUPLICATE", Astra::Core::DiagnosticSeverity::Blocking, "Duplicate AssetId found in sidecar registry scan.", entry.path());
            diagnostic.objects.push_back({"AssetId", registry_entry.id.ToString()});
            registry_entry.diagnostics.push_back(diagnostic);
            diagnostics.Emit(diagnostic);
        }
        registry_entry.hard_dependencies = sidecar.Value().hard_dependencies;
        registry_entry.soft_dependencies = sidecar.Value().soft_dependencies;
        registry.entries.push_back(std::move(registry_entry));
    }
    std::set<std::string> known;
    for (const auto& entry : registry.entries) {
        known.insert(entry.id.ToString());
    }
    for (auto& entry : registry.entries) {
        for (const auto& dependency : entry.hard_dependencies) {
            if (!known.contains(dependency.ToString())) {
                auto diagnostic = MakeDiagnostic("ASTRA_ASSET_DEPENDENCY_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Hard asset dependency is missing.", entry.sidecar_path);
                diagnostic.objects.push_back({"AssetId", entry.id.ToString()});
                diagnostic.objects.push_back({"MissingAssetId", dependency.ToString()});
                entry.diagnostics.push_back(diagnostic);
                diagnostics.Emit(std::move(diagnostic));
            }
        }
    }
    return registry;
}

std::vector<AssetInvalidation> AssetRegistryBuilder::WatchContent(Astra::Platform::IFileSystemService& filesystem, const std::filesystem::path& content_root) const {
    std::vector<AssetInvalidation> invalidations;
    filesystem.Watch(content_root, [&](const std::filesystem::path& changed) {
        invalidations.push_back({changed, {}, "platform_watch"});
    });
    return invalidations;
}

Astra::Core::Result<void> ValidateImportPreset(const ImportPresetDescriptor& descriptor, Astra::Core::DiagnosticSink& diagnostics) {
    if (descriptor.id.empty() || descriptor.source_extensions.empty() || descriptor.asset_type.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_IMPORT_PRESET_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Import preset requires id, source extensions and asset type."));
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "invalid import preset");
    }
    return Astra::Core::Result<void>::Success();
}

Astra::Core::Result<void> ValidateProjectTemplate(const ProjectTemplateDescriptor& descriptor, Astra::Core::DiagnosticSink& diagnostics) {
    if (descriptor.id.empty() || descriptor.display_name.empty() || descriptor.runtime_profile.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PROJECT_TEMPLATE_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Project template requires id, display name and runtime profile."));
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "invalid project template");
    }
    return Astra::Core::Result<void>::Success();
}

Astra::Core::Result<void> ValidateReviewQueueItem(const ReviewQueueItem& item, Astra::Core::DiagnosticSink& diagnostics) {
    if (item.id.empty() || item.kind.empty() || item.source_ref.empty() || item.target_ref.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_REVIEW_ITEM_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Review item requires id, kind, source ref and target ref."));
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "invalid review queue item");
    }
    return Astra::Core::Result<void>::Success();
}

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

} // namespace Astra::Asset
