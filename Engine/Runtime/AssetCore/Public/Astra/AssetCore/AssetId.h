#pragma once

#include <Astra/Core/Result.h>

#include <string>

namespace astra {

class AssetId {
  public:
    AssetId() = default;
    explicit AssetId(std::string value);

    [[nodiscard]] static Expected<AssetId> parse(std::string value);
    [[nodiscard]] const std::string& value() const;
    [[nodiscard]] std::string scheme() const;
    [[nodiscard]] bool empty() const;

    friend bool operator==(const AssetId& lhs, const AssetId& rhs) = default;

  private:
    std::string value_;
};

} // namespace astra

template <> struct std::hash<astra::AssetId> {
    std::size_t operator()(const astra::AssetId& id) const noexcept {
        return std::hash<std::string>{}(id.value());
    }
};
