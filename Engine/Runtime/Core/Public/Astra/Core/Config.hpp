#pragma once

#include <Astra/Core/Export.hpp>
#include <Astra/Core/Types.hpp>
#include <nlohmann/json.hpp>

#include <string_view>
#include <vector>

namespace Astra::Core {

enum class ConfigScope {
    EngineDefault,
    PlatformDefault,
    Project,
    RuntimeProfile,
    ReleaseProfile,
    UserOverride,
    CommandLine
};

enum class ConfigResolveProfile {
    Development,
    Runtime,
    Release
};

struct ConfigLayer {
    ConfigScope scope = ConfigScope::EngineDefault;
    nlohmann::json values;
};

struct ResolvedConfig {
    ConfigResolveProfile profile = ConfigResolveProfile::Development;
    nlohmann::json values = nlohmann::json::object();
    std::string hash;
    bool user_overrides_included = true;
};

class ASTRA_CORE_API ConfigStack {
public:
    void AddLayer(ConfigLayer layer);
    [[nodiscard]] nlohmann::json Resolve(bool include_user_overrides = true) const;
    [[nodiscard]] std::string Hash(bool include_user_overrides = true) const;
    [[nodiscard]] ResolvedConfig ResolveForProfile(ConfigResolveProfile profile) const;
    [[nodiscard]] std::string HashForProfile(ConfigResolveProfile profile) const;

private:
    std::vector<ConfigLayer> layers_;
};

ASTRA_CORE_API void MergeJsonObject(nlohmann::json& target, const nlohmann::json& overlay);
[[nodiscard]] ASTRA_CORE_API std::string ToString(ConfigScope scope);
[[nodiscard]] ASTRA_CORE_API std::string ToString(ConfigResolveProfile profile);
[[nodiscard]] ASTRA_CORE_API nlohmann::json ToJson(const ResolvedConfig& config);

} // namespace Astra::Core
