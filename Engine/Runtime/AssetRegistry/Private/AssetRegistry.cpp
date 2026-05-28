#include <Astra/AssetRegistry/AssetRegistry.h>

#include <Astra/Core/Path.h>

#include <yaml-cpp/yaml.h>

#include <filesystem>

namespace astra {

namespace {

std::string required_string(const YAML::Node& node, const char* field) {
    if (!node[field] || !node[field].IsScalar()) {
        throw std::runtime_error(std::string("missing required field: ") + field);
    }
    return node[field].as<std::string>();
}

} // namespace

VoidResult AssetRegistry::load_sidecars(const std::filesystem::path& content_root,
                                        DiagnosticSink& diagnostics) {
    assets_.clear();
    if (!std::filesystem::exists(content_root)) {
        return std::unexpected(
            make_error("asset_registry.content_missing",
                       "Content root does not exist: " + path_to_utf8(content_root)));
    }

    for (const auto& entry : std::filesystem::recursive_directory_iterator(content_root)) {
        if (!entry.is_regular_file()) {
            continue;
        }
        const auto path = entry.path();
        if (path.filename().string().ends_with(".asset.yaml")) {
            try {
                const YAML::Node root = YAML::LoadFile(path_to_utf8(path));
                auto id_result = AssetId::parse(required_string(root, "id"));
                if (!id_result) {
                    diagnostics.error(id_result.error().code,
                                      path_to_utf8(path) + ": " + id_result.error().message);
                    continue;
                }

                AssetMetadata metadata;
                metadata.id = *id_result;
                metadata.type = asset_type_from_string(required_string(root, "type"));
                metadata.source_path = std::filesystem::path(required_string(root, "source_path"));
                metadata.origin = root["origin"] ? root["origin"].as<std::string>() : "unknown";
                metadata.display_name = root["display_name"]
                                            ? root["display_name"].as<std::string>()
                                            : metadata.id.value();
                if (root["tags"] && root["tags"].IsSequence()) {
                    for (const YAML::Node& tag : root["tags"]) {
                        metadata.tags.push_back(tag.as<std::string>());
                    }
                }

                if (assets_.contains(metadata.id)) {
                    diagnostics.error("asset_registry.duplicate_id",
                                      "Duplicate AssetId: " + metadata.id.value());
                    continue;
                }
                assets_.emplace(metadata.id, std::move(metadata));
            } catch (const std::exception& ex) {
                diagnostics.error("asset_registry.sidecar_parse",
                                  path_to_utf8(path) + ": " + ex.what());
            }
        }
    }

    if (diagnostics.has_errors()) {
        return std::unexpected(make_error("asset_registry.invalid_sidecars",
                                          "One or more asset sidecars failed validation"));
    }
    return {};
}

std::optional<AssetMetadata> AssetRegistry::find(const AssetId& id) const {
    const auto it = assets_.find(id);
    if (it == assets_.end()) {
        return std::nullopt;
    }
    return it->second;
}

std::vector<AssetMetadata> AssetRegistry::list() const {
    std::vector<AssetMetadata> result;
    result.reserve(assets_.size());
    for (const auto& [_, metadata] : assets_) {
        result.push_back(metadata);
    }
    return result;
}

std::size_t AssetRegistry::size() const {
    return assets_.size();
}

} // namespace astra
