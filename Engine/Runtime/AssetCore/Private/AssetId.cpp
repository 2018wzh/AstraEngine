#include <Astra/AssetCore/AssetId.h>
#include <Astra/AssetCore/AssetMetadata.h>

#include <algorithm>

namespace astra {

AssetId::AssetId(std::string value) : value_(std::move(value)) {}

Expected<AssetId> AssetId::parse(std::string value) {
    const auto delimiter = value.find(":/");
    if (delimiter == std::string::npos || delimiter == 0 || delimiter + 2 >= value.size()) {
        return std::unexpected(
            make_error("asset.invalid_id", "AssetId must use '<scheme>:/<path>' syntax"));
    }
    if (value.find('\\') != std::string::npos) {
        return std::unexpected(
            make_error("asset.invalid_separator", "AssetId must use forward slashes"));
    }
    return AssetId{std::move(value)};
}

const std::string& AssetId::value() const {
    return value_;
}

std::string AssetId::scheme() const {
    const auto delimiter = value_.find(":/");
    if (delimiter == std::string::npos) {
        return {};
    }
    return value_.substr(0, delimiter);
}

bool AssetId::empty() const {
    return value_.empty();
}

AssetType asset_type_from_string(std::string_view value) {
    if (value == "image") {
        return AssetType::Image;
    }
    if (value == "audio") {
        return AssetType::Audio;
    }
    if (value == "font") {
        return AssetType::Font;
    }
    if (value == "script") {
        return AssetType::Script;
    }
    return AssetType::Unknown;
}

std::string asset_type_to_string(AssetType type) {
    switch (type) {
    case AssetType::Image:
        return "image";
    case AssetType::Audio:
        return "audio";
    case AssetType::Font:
        return "font";
    case AssetType::Script:
        return "script";
    case AssetType::Unknown:
        return "unknown";
    }
    return "unknown";
}

} // namespace astra
