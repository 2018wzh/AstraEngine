#pragma once

#include <Astra/AssetCore/AssetMetadata.h>
#include <Astra/Core/Diagnostics.h>
#include <Astra/Core/Result.h>

#include <filesystem>
#include <optional>
#include <unordered_map>
#include <vector>

namespace astra {

class AssetRegistry {
  public:
    VoidResult load_sidecars(const std::filesystem::path& content_root,
                             DiagnosticSink& diagnostics);
    [[nodiscard]] std::optional<AssetMetadata> find(const AssetId& id) const;
    [[nodiscard]] std::vector<AssetMetadata> list() const;
    [[nodiscard]] std::size_t size() const;

  private:
    std::unordered_map<AssetId, AssetMetadata> assets_;
};

} // namespace astra
