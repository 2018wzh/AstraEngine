#include <Astra/Asset/Asset.hpp>

#include "AssetInternal.hpp"

#include <openssl/evp.h>
#include <yaml-cpp/yaml.h>
#include <algorithm>
#include <array>
#include <csetjmp>
#include <cctype>
#include <cstddef>
#include <cstring>
#include <fstream>
#include <iomanip>
#include <iterator>
#include <limits>
#include <set>
#include <sstream>
#include <zstd.h>

namespace Astra::Asset {

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

Astra::Core::Diagnostic MakeDiagnostic(std::string code, Astra::Core::DiagnosticSeverity severity, std::string message, const std::filesystem::path& path) {
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

std::vector<Astra::Core::u8> ReadBytes(const std::filesystem::path& path) {
    std::ifstream file(path, std::ios::binary);
    const std::string bytes(std::istreambuf_iterator<char>(file), {});
    return {bytes.begin(), bytes.end()};
}

void WriteBytes(const std::filesystem::path& path, std::span<const Astra::Core::u8> bytes) {
    if (path.has_parent_path()) {
        std::filesystem::create_directories(path.parent_path());
    }
    std::ofstream file(path, std::ios::binary);
    if (!bytes.empty()) {
        file.write(reinterpret_cast<const char*>(bytes.data()), static_cast<std::streamsize>(bytes.size()));
    }
}

std::string LowerExtension(const std::filesystem::path& path) {
    auto extension = path.extension().string();
    std::ranges::transform(extension, extension.begin(), [](unsigned char character) {
        return static_cast<char>(std::tolower(character));
    });
    return extension;
}

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

std::string Sha256Span(std::span<const Astra::Core::u8> bytes) {
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

std::string Sha256File(const std::filesystem::path& path) {
    return Sha256Span(ReadBytes(path));
}

std::vector<Astra::Core::u8> CompressZstd(std::span<const Astra::Core::u8> bytes) {
    std::vector<Astra::Core::u8> output(ZSTD_compressBound(bytes.size()));
    const auto size = ZSTD_compress(output.data(), output.size(), bytes.data(), bytes.size(), 3);
    if (ZSTD_isError(size)) {
        return {};
    }
    output.resize(size);
    return output;
}

Astra::Core::Result<std::vector<Astra::Core::u8>> DecompressZstd(std::span<const Astra::Core::u8> bytes, Astra::Core::u64 expected_size) {
    std::vector<Astra::Core::u8> output(static_cast<std::size_t>(expected_size));
    const auto size = ZSTD_decompress(output.data(), output.size(), bytes.data(), bytes.size());
    if (ZSTD_isError(size) || size != expected_size) {
        return Astra::Core::Result<std::vector<Astra::Core::u8>>::Failure(Astra::Core::ErrorCode::InvalidFormat, "could not decompress zstd package payload");
    }
    return Astra::Core::Result<std::vector<Astra::Core::u8>>::Success(std::move(output));
}

template <typename T>
void WritePod(std::ofstream& file, T value) {
    file.write(reinterpret_cast<const char*>(&value), sizeof(T));
}

template <typename T>
bool ReadPod(std::ifstream& file, T& value) {
    file.read(reinterpret_cast<char*>(&value), sizeof(T));
    return static_cast<bool>(file);
}

constexpr std::array<char, 8> kPackageMagic = {'A', 'S', 'T', 'R', 'A', 'P', '6', '\0'};
constexpr Astra::Core::u32 kPackageVersion = 1;

Astra::Core::u64 BinaryPackageHeaderSize() {
    return static_cast<Astra::Core::u64>(kPackageMagic.size() + sizeof(Astra::Core::u32) + sizeof(Astra::Core::u64));
}

Astra::Core::Result<nlohmann::json> ReadPackageJson(const std::filesystem::path& package_path, Astra::Core::DiagnosticSink& diagnostics) {
    std::ifstream file(package_path, std::ios::binary);
    if (!file) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_MANIFEST_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Package manifest could not be opened.", package_path));
        return Astra::Core::Result<nlohmann::json>::Failure(Astra::Core::ErrorCode::NotFound, "package manifest missing");
    }
    std::array<char, 8> magic{};
    file.read(magic.data(), static_cast<std::streamsize>(magic.size()));
    file.seekg(0);
    if (magic == kPackageMagic) {
        std::array<char, 8> read_magic{};
        Astra::Core::u32 version = 0;
        Astra::Core::u64 manifest_size = 0;
        file.read(read_magic.data(), static_cast<std::streamsize>(read_magic.size()));
        if (!ReadPod(file, version) || !ReadPod(file, manifest_size) || version != kPackageVersion || manifest_size == 0) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_HEADER_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Binary package header is invalid.", package_path));
            return Astra::Core::Result<nlohmann::json>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package header invalid");
        }
        std::string manifest_text(static_cast<std::size_t>(manifest_size), '\0');
        file.read(manifest_text.data(), static_cast<std::streamsize>(manifest_text.size()));
        if (!file) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_MANIFEST_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Binary package manifest is truncated.", package_path));
            return Astra::Core::Result<nlohmann::json>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package manifest truncated");
        }
        auto json = nlohmann::json::parse(manifest_text, nullptr, false);
        if (json.is_discarded()) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_MANIFEST_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Binary package manifest JSON is invalid.", package_path));
            return Astra::Core::Result<nlohmann::json>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package manifest invalid");
        }
        return Astra::Core::Result<nlohmann::json>::Success(std::move(json));
    }

    auto json = nlohmann::json::parse(file, nullptr, false);
    if (json.is_discarded()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_PACKAGE_MANIFEST_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Package manifest JSON is invalid.", package_path));
        return Astra::Core::Result<nlohmann::json>::Failure(Astra::Core::ErrorCode::InvalidFormat, "package manifest invalid");
    }
    return Astra::Core::Result<nlohmann::json>::Success(std::move(json));
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

std::string AssetUri::ToString() const {
    std::string output = scheme_name + ":/" + NormalizePath(path);
    if (!fragment.empty()) {
        output += "#" + fragment;
    }
    return output;
}

Astra::Core::Result<void> ValidateImporterDescriptor(const ImporterDescriptor& descriptor, Astra::Core::DiagnosticSink& diagnostics) {
    bool valid = true;
    if (descriptor.provider_id.empty() || descriptor.contract != "IAssetImporter") {
        diagnostics.Emit(MakeDiagnostic("ASTRA_IMPORTER_DESCRIPTOR_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Importer descriptor requires provider_id and IAssetImporter contract."));
        valid = false;
    }
    if (descriptor.source_extensions.empty() || descriptor.output_asset_types.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_IMPORTER_DESCRIPTOR_FORMATS", Astra::Core::DiagnosticSeverity::Blocking, "Importer descriptor requires source extensions and output asset types."));
        valid = false;
    }
    if (descriptor.sidecar_schema != AssetSidecarSchema) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_IMPORTER_SIDECAR_SCHEMA", Astra::Core::DiagnosticSeverity::Blocking, "Importer descriptor uses unsupported sidecar schema."));
        valid = false;
    }
    return valid ? Astra::Core::Result<void>::Success() : Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "invalid importer descriptor");
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

std::string DdcKey::ToString() const {
    nlohmann::json key = {
        {"schema", schema},
        {"asset_id", asset_id.ToString()},
        {"source_hash", source_hash},
        {"sidecar_hash", sidecar_hash},
        {"processor_id", processor_id},
        {"processor_version", processor_version},
        {"platform", platform},
        {"profile", profile},
        {"provider_feature_hash", provider_feature_hash},
    };
    return "ddc:/" + processor_id + "/" + Sha256Text(key.dump());
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
    if (!json.contains("id")) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_SIDECAR_REQUIRED", Astra::Core::DiagnosticSeverity::Blocking, "Asset sidecar requires id.", sidecar_path));
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
    sidecar.requires_review = JsonValueAsBool(json, "requires_review", false);
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
        Astra::Core::Result<AssetSidecar> sidecar =
            Astra::Core::Result<AssetSidecar>::Failure(Astra::Core::ErrorCode::InvalidFormat, "asset sidecar not loaded");
        const auto diagnostic_start = diagnostics.Diagnostics().size();
        try {
            const auto json = YamlToJson(YAML::LoadFile(entry.path().string()));
            sidecar = AssetSidecarFromJson(json, entry.path(), diagnostics);
        } catch (const YAML::Exception& error) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_ASSET_SIDECAR_PARSE", Astra::Core::DiagnosticSeverity::Blocking, error.what(), entry.path()));
        }
        if (!sidecar) {
            continue;
        }
        AssetRegistryEntry registry_entry;
        (void)ValidateAssetSidecar(sidecar.Value(), entry.path(), diagnostics);
        registry_entry.id = sidecar.Value().id;
        registry_entry.type = sidecar.Value().type;
        registry_entry.sidecar_path = entry.path();
        registry_entry.source_path = entry.path().parent_path() / sidecar.Value().source_path;
        registry_entry.sidecar_hash = Sha256Text(ReadText(entry.path()));
        registry_entry.origin = sidecar.Value().origin;
        registry_entry.license = sidecar.Value().license;
        registry_entry.review = sidecar.Value().review;
        registry_entry.requires_review = sidecar.Value().requires_review;
        registry_entry.has_ai_generation = sidecar.Value().ai_generation.has_value();
        const auto& validation_diagnostics = diagnostics.Diagnostics();
        for (std::size_t index = diagnostic_start; index < validation_diagnostics.size(); ++index) {
            registry_entry.diagnostics.push_back(validation_diagnostics[index]);
        }
        if (!sidecar.Value().source_path.empty() && std::filesystem::is_regular_file(registry_entry.source_path)) {
            registry_entry.source_hash = Sha256File(registry_entry.source_path);
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

std::vector<AssetInvalidation> AssetRegistryBuilder::ComputeInvalidations(const AssetRegistry& before, const AssetRegistry& after) const {
    std::map<std::string, AssetRegistryEntry> previous;
    for (const auto& entry : before.entries) {
        previous[entry.id.ToString()] = entry;
    }
    std::vector<AssetInvalidation> invalidations;
    for (const auto& entry : after.entries) {
        const auto id = entry.id.ToString();
        const auto found = previous.find(id);
        if (found == previous.end()) {
            invalidations.push_back({entry.source_path, {entry.id}, "asset_added"});
            continue;
        }
        if (found->second.source_hash != entry.source_hash || found->second.sidecar_hash != entry.sidecar_hash) {
            invalidations.push_back({entry.source_path, {entry.id}, "asset_changed"});
        }
    }
    std::set<std::string> after_ids;
    for (const auto& entry : after.entries) {
        after_ids.insert(entry.id.ToString());
    }
    for (const auto& entry : before.entries) {
        if (!after_ids.contains(entry.id.ToString())) {
            invalidations.push_back({entry.source_path, {entry.id}, "asset_removed"});
        }
    }
    return invalidations;
}

Astra::Core::Result<void> ValidateImportPreset(const ImportPresetDescriptor& descriptor, Astra::Core::DiagnosticSink& diagnostics) {
    if (descriptor.id.empty() || descriptor.source_extensions.empty() || descriptor.asset_type.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_IMPORT_PRESET_INVALID", Astra::Core::DiagnosticSeverity::Blocking, "Import preset requires id, source extensions and asset type."));
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "invalid import preset");
    }
    return Astra::Core::Result<void>::Success();
}

Astra::Core::Result<void> ValidateImportRequest(const ImportRequest& request, Astra::Core::DiagnosticSink& diagnostics) {
    bool valid = true;
    if (request.schema != ImportRequestSchema) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_IMPORT_REQUEST_SCHEMA", Astra::Core::DiagnosticSeverity::Blocking, "Import request schema is unsupported.", request.source_path));
        valid = false;
    }
    if (request.source_path.empty() || !std::filesystem::exists(request.source_path)) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_IMPORT_SOURCE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Import source file is missing.", request.source_path));
        valid = false;
    }
    if (request.target_asset_id.scheme != AssetScheme::Native && request.target_asset_id.scheme != AssetScheme::Foreign) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_IMPORT_TARGET_SCHEME", Astra::Core::DiagnosticSeverity::Blocking, "Import target must be native:/ or foreign-*:/.", request.source_path));
        valid = false;
    }
    if (request.asset_type.empty() || request.preset.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_IMPORT_REQUEST_REQUIRED", Astra::Core::DiagnosticSeverity::Blocking, "Import request requires asset_type and preset.", request.source_path));
        valid = false;
    }
    if (request.license.owner.empty() || request.license.usage.empty()) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_IMPORT_LICENSE_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Import request requires license owner and usage.", request.source_path));
        valid = false;
    }
    if ((request.origin == "AIGenerated" || request.review_state != "accepted") && request.review_state != "accepted") {
        diagnostics.Emit(MakeDiagnostic("ASTRA_RELEASE_ASSET_004", Astra::Core::DiagnosticSeverity::Blocking, "Unreviewed AI asset cannot be imported.", request.source_path));
        valid = false;
    }
    if (request.target_asset_id.scheme == AssetScheme::Foreign && request.copy_source && !request.foreign_copy_allowed) {
        diagnostics.Emit(MakeDiagnostic("ASTRA_RELEASE_ASSET_FOREIGN_COPY", Astra::Core::DiagnosticSeverity::Blocking, "Foreign assets are mount-only unless foreign copy is explicitly allowed.", request.source_path));
        valid = false;
    }
    return valid ? Astra::Core::Result<void>::Success() : Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "invalid import request");
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

} // namespace Astra::Asset
