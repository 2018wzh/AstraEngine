#pragma once

#include <Astra/AssetCore/AssetId.h>

#include <filesystem>
#include <string>
#include <vector>

namespace astra {

enum class AssetType {
    Image,
    Audio,
    Font,
    Script,
    Unknown,
};

struct AssetMetadata {
    AssetId id;
    AssetType type = AssetType::Unknown;
    std::filesystem::path source_path;
    std::string display_name;
    std::string origin;
    std::vector<std::string> tags;
};

AssetType asset_type_from_string(std::string_view value);
std::string asset_type_to_string(AssetType type);

} // namespace astra
