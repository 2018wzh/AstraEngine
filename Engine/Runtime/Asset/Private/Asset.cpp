#include <Astra/Asset/Asset.hpp>

#include <openssl/evp.h>
#include <yaml-cpp/yaml.h>

#include <algorithm>
#include <array>
#include <cstddef>
#include <fstream>
#include <iomanip>
#include <iterator>
#include <limits>
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

std::string Sha256Text(std::string_view text) {
    EVP_MD_CTX* context = EVP_MD_CTX_new();
    EVP_DigestInit_ex(context, EVP_sha256(), nullptr);
    EVP_DigestUpdate(context, text.data(), text.size());
    std::array<unsigned char, EVP_MAX_MD_SIZE> digest{};
    unsigned int digest_size = 0;
    EVP_DigestFinal_ex(context, digest.data(), &digest_size);
    EVP_MD_CTX_free(context);

    std::ostringstream output;
    for (unsigned int index = 0; index < digest_size; ++index) {
        output << std::hex << std::setw(2) << std::setfill('0') << static_cast<int>(digest[index]);
    }
    return output.str();
}

Astra::Core::Result<std::vector<Astra::Core::u8>> DecodeBase64(std::string_view text) {
    if (text.empty()) {
        return Astra::Core::Result<std::vector<Astra::Core::u8>>::Success({});
    }
    if (text.size() > static_cast<std::size_t>(std::numeric_limits<int>::max())) {
        return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(Astra::Core::ErrorCode::InvalidFormat, "base64 payload is too large");
    }
    std::vector<Astra::Core::u8> decoded(((text.size() + 3) / 4) * 3);
    const auto decoded_size = EVP_DecodeBlock(decoded.data(), reinterpret_cast<const unsigned char*>(text.data()), static_cast<int>(text.size()));
    if (decoded_size < 0) {
        return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(Astra::Core::ErrorCode::InvalidFormat, "base64 payload is invalid");
    }
    auto size = static_cast<std::size_t>(decoded_size);
    if (!text.empty() && text.back() == '=') {
        --size;
    }
    if (text.size() >= 2 && text[text.size() - 2] == '=') {
        --size;
    }
    decoded.resize(size);
    return Astra::Core::Result<std::vector<Astra::Core::u8>>::Success(std::move(decoded));
}

std::string Sha256Bytes(const std::vector<Astra::Core::u8>& bytes) {
    EVP_MD_CTX* context = EVP_MD_CTX_new();
    EVP_DigestInit_ex(context, EVP_sha256(), nullptr);
    if (!bytes.empty()) {
        EVP_DigestUpdate(context, bytes.data(), bytes.size());
    }
    std::array<unsigned char, EVP_MAX_MD_SIZE> digest{};
    unsigned int digest_size = 0;
    EVP_DigestFinal_ex(context, digest.data(), &digest_size);
    EVP_MD_CTX_free(context);

    std::ostringstream output;
    for (unsigned int index = 0; index < digest_size; ++index) {
        output << std::hex << std::setw(2) << std::setfill('0') << static_cast<int>(digest[index]);
    }
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
        {"assets", assets},
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
    if (json.contains("cook_manifest")) {
        const auto& cook = json.at("cook_manifest");
        manifest.cook_manifest.project_id = cook.value("project_id", "");
        manifest.cook_manifest.profile = cook.value("profile", "");
        manifest.cook_manifest.manifest_hash = cook.value("manifest_hash", "");
        for (const auto& artifact_json : cook.value("artifacts", nlohmann::json::array())) {
            auto asset_id = ParseAssetUri(artifact_json.value("asset_id", "asset:/invalid"));
            if (!asset_id) {
                continue;
            }
            CookArtifact artifact;
            artifact.asset_id = asset_id.Value();
            artifact.type = artifact_json.value("type", "");
            artifact.artifact_path = artifact_json.value("artifact_path", "");
            artifact.hash = artifact_json.value("hash", "");
            artifact.processor_id = artifact_json.value("processor_id", "");
            artifact.ddc_key = artifact_json.value("ddc_key", "");
            artifact.metadata = artifact_json.value("metadata", nlohmann::json::object());
            manifest.cook_manifest.artifacts.push_back(std::move(artifact));
        }
        for (const auto& ddc_json : cook.value("ddc_entries", nlohmann::json::array())) {
            DerivedDataCacheEntry entry;
            entry.schema = ddc_json.value("schema", DerivedDataCacheEntrySchema);
            entry.key = ddc_json.value("key", "");
            entry.processor_id = ddc_json.value("processor_id", "");
            entry.input_hash = ddc_json.value("input_hash", "");
            entry.settings_hash = ddc_json.value("settings_hash", "");
            entry.output_hash = ddc_json.value("output_hash", "");
            entry.platform = ddc_json.value("platform", "win64");
            entry.profile = ddc_json.value("profile", "");
            entry.engine_version = ddc_json.value("engine_version", "");
            entry.rebuilt = ddc_json.value("rebuilt", false);
            entry.corruption_recovered = ddc_json.value("corruption_recovered", false);
            for (const auto& artifact_json : ddc_json.value("artifacts", nlohmann::json::array())) {
                entry.artifacts.push_back({
                    artifact_json.value("path", ""),
                    artifact_json.value("hash", ""),
                    artifact_json.value("size_bytes", static_cast<Astra::Core::u64>(0)),
                });
            }
            manifest.cook_manifest.ddc_entries.push_back(std::move(entry));
        }
    }
    for (const auto& module_json : json.value("modules", nlohmann::json::array())) {
        manifest.modules.push_back({
            module_json.value("id", ""),
            module_json.value("binary", ""),
            module_json.value("sha256", ""),
            module_json.value("runtime_safe", true),
        });
    }
    for (const auto& payload_json : json.value("payloads", nlohmann::json::array())) {
        auto asset_id = ParseAssetUri(payload_json.value("asset_id", "asset:/invalid"));
        if (!asset_id) {
            continue;
        }
        PackagePayloadEntry payload;
        payload.asset_id = asset_id.Value();
        payload.artifact_path = payload_json.value("artifact_path", "");
        payload.hash = payload_json.value("hash", "");
        payload.size_bytes = payload_json.value("size_bytes", static_cast<Astra::Core::u64>(0));
        payload.encoding = payload_json.value("encoding", "base64");
        payload.data = payload_json.value("data", "");
        manifest.payloads.push_back(std::move(payload));
    }
    manifest.runtime_evidence = json.value("runtime_evidence", nlohmann::json::object());
    return Astra::Core::Result<PackageManifest>::Success(std::move(manifest));
}

std::string ComputeCookManifestHash(nlohmann::json manifest_json) {
    manifest_json["manifest_hash"] = "";
    return Sha256Text(manifest_json.dump());
}

std::string ComputePackageManifestHash(nlohmann::json manifest_json) {
    manifest_json["package_hash"] = "";
    return Sha256Text(manifest_json.dump());
}

Astra::Core::Result<PackageManifest> PackageReader::ReadManifest(const std::filesystem::path& package_path, Astra::Core::DiagnosticSink& diagnostics) const {
    std::ifstream file(package_path, std::ios::binary);
    if (!file) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_MANIFEST_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Package manifest could not be opened.", package_path));
        return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::NotFound, "package manifest missing");
    }
    auto json = nlohmann::json::parse(file, nullptr, false);
    if (json.is_discarded()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_MANIFEST_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Package manifest JSON is invalid.", package_path));
        return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package manifest invalid");
    }
    auto manifest = PackageManifestFromJson(json);
    if (!manifest) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_MANIFEST_SCHEMA", Astra::Core::DiagnosticSeverity::Blocking, manifest.Message(), package_path));
        return manifest;
    }

    const auto expected_package_hash = json.value("package_hash", "");
    const auto actual_package_hash = ComputePackageManifestHash(json);
    if (expected_package_hash.empty() || expected_package_hash != actual_package_hash) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Package manifest hash does not match package contents.", package_path));
        return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package hash mismatch");
    }

    if (json.contains("cook_manifest")) {
        const auto& cook_json = json.at("cook_manifest");
        const auto expected_cook_hash = cook_json.value("manifest_hash", "");
        const auto actual_cook_hash = ComputeCookManifestHash(cook_json);
        if (expected_cook_hash.empty() || expected_cook_hash != actual_cook_hash) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_COOK_MANIFEST_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Cook manifest hash does not match cook artifacts and DDC metadata.", package_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "cook manifest hash mismatch");
        }
    }

    std::map<std::string, DerivedDataCacheEntry> ddc_by_key;
    for (const auto& entry : manifest.Value().cook_manifest.ddc_entries) {
        if (entry.key.empty() || entry.output_hash.empty() || entry.processor_id.empty()) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_DDC_ENTRY_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "DDC entry is missing key, processor, or output hash.", package_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "ddc entry invalid");
        }
        ddc_by_key[entry.key] = entry;
    }
    for (const auto& artifact : manifest.Value().cook_manifest.artifacts) {
        if (artifact.ddc_key.empty()) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_DDC_ARTIFACT_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Cook artifact is missing its DDC key.", package_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "artifact ddc key missing");
        }
        const auto found = ddc_by_key.find(artifact.ddc_key);
        if (found == ddc_by_key.end() || found->second.output_hash != artifact.hash) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_DDC_ARTIFACT_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Cook artifact hash does not match DDC metadata.", package_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "artifact ddc hash mismatch");
        }
    }

    std::map<std::string, CookArtifact> artifacts_by_asset;
    for (const auto& artifact : manifest.Value().cook_manifest.artifacts) {
        artifacts_by_asset[artifact.asset_id.ToString()] = artifact;
    }
    std::set<std::string> payload_assets;
    for (const auto& payload : manifest.Value().payloads) {
        const auto asset = payload.asset_id.ToString();
        if (payload.encoding != "base64") {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_ENCODING", Astra::Core::DiagnosticSeverity::Blocking, "Package payload uses an unsupported encoding.", package_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload encoding unsupported");
        }
        auto decoded = DecodeBase64(payload.data);
        if (!decoded) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_ENCODING", Astra::Core::DiagnosticSeverity::Blocking, decoded.Message(), package_path));
            return Astra::Core::Result<PackageManifest>::Failure(decoded.Error(), decoded.Message());
        }
        if (decoded.Value().size() != payload.size_bytes) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_SIZE_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Package payload decoded size does not match manifest metadata.", package_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload size mismatch");
        }
        const auto payload_hash = Sha256Bytes(decoded.Value());
        if (payload.hash.empty() || payload_hash != payload.hash) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Package payload hash does not match decoded bytes.", package_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload hash mismatch");
        }
        const auto artifact = artifacts_by_asset.find(asset);
        if (artifact != artifacts_by_asset.end() && artifact->second.hash != payload.hash) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_HASH_MISMATCH", Astra::Core::DiagnosticSeverity::Blocking, "Package payload hash does not match the cook artifact hash.", package_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload artifact hash mismatch");
        }
        payload_assets.insert(asset);
    }
    for (const auto& artifact : manifest.Value().cook_manifest.artifacts) {
        if (!payload_assets.contains(artifact.asset_id.ToString())) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Cook artifact has no embedded package payload.", package_path));
            return Astra::Core::Result<PackageManifest>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package payload missing");
        }
    }
    return manifest;
}

Astra::Core::Result<std::vector<Astra::Core::u8>> PackageReader::ReadPayloadBytes(const std::filesystem::path& package_path, const AssetUri& asset_id, Astra::Core::DiagnosticSink& diagnostics) const {
    auto manifest = ReadManifest(package_path, diagnostics);
    if (!manifest) {
        return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(manifest.Error(), manifest.Message());
    }
    const auto requested = asset_id.ToString();
    const auto found = std::ranges::find_if(manifest.Value().payloads, [&](const PackagePayloadEntry& payload) {
        return payload.asset_id.ToString() == requested;
    });
    if (found == manifest.Value().payloads.end()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_NOT_FOUND", Astra::Core::DiagnosticSeverity::Blocking, "Requested asset payload is not present in the package.", package_path));
        return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(Astra::Core::ErrorCode::NotFound, "package payload not found");
    }
    auto decoded = DecodeBase64(found->data);
    if (!decoded) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_ENCODING", Astra::Core::DiagnosticSeverity::Blocking, decoded.Message(), package_path));
        return decoded;
    }
    return decoded;
}

Astra::Core::Result<std::vector<PackagePayloadChunk>> PackageReader::ReadPayloadChunks(const std::filesystem::path& package_path, const AssetUri& asset_id, Astra::Core::u64 chunk_size, Astra::Core::DiagnosticSink& diagnostics) const {
    if (chunk_size == 0) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_PAYLOAD_CHUNK_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Package payload chunk size must be greater than zero.", package_path));
        return Astra::Core::Result<std::vector<PackagePayloadChunk>>::Failure(Astra::Core::ErrorCode::InvalidArgument, "package payload chunk size invalid");
    }
    auto bytes = ReadPayloadBytes(package_path, asset_id, diagnostics);
    if (!bytes) {
        return Astra::Core::Result<std::vector<PackagePayloadChunk>>::Failure(bytes.Error(), bytes.Message());
    }
    std::vector<PackagePayloadChunk> chunks;
    for (Astra::Core::u64 offset = 0; offset < bytes.Value().size(); offset += chunk_size) {
        const auto remaining = static_cast<Astra::Core::u64>(bytes.Value().size()) - offset;
        const auto count = static_cast<std::size_t>(std::min(chunk_size, remaining));
        const auto begin = bytes.Value().begin() + static_cast<std::ptrdiff_t>(offset);
        PackagePayloadChunk chunk;
        chunk.asset_id = asset_id;
        chunk.offset = offset;
        chunk.bytes.assign(begin, begin + static_cast<std::ptrdiff_t>(count));
        chunk.final_chunk = offset + count == bytes.Value().size();
        chunks.push_back(std::move(chunk));
    }
    if (bytes.Value().empty()) {
        chunks.push_back({asset_id, 0, {}, true});
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
    mount.package_path = std::filesystem::absolute(package_path).lexically_normal();
    for (const auto& payload : manifest.Value().payloads) {
        mount.assets.push_back(payload.asset_id);
    }
    return Astra::Core::Result<PackageMount>::Success(std::move(mount));
}

} // namespace Astra::Asset
