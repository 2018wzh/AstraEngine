#include <Astra/Core/Config.hpp>

#include <iomanip>
#include <sstream>

namespace Astra::Core {

namespace {

bool IncludeScope(ConfigResolveProfile profile, ConfigScope scope) {
    if (profile == ConfigResolveProfile::Development) {
        return true;
    }
    if (scope == ConfigScope::UserOverride) {
        return false;
    }
    if (profile == ConfigResolveProfile::Release && scope == ConfigScope::RuntimeProfile) {
        return true;
    }
    return true;
}

std::string StableHash(std::string_view text) {
    Astra::Core::u64 hash = 14695981039346656037ull;
    for (const auto ch : text) {
        hash ^= static_cast<unsigned char>(ch);
        hash *= 1099511628211ull;
    }
    std::ostringstream stream;
    stream << std::hex << std::setw(16) << std::setfill('0') << hash;
    return stream.str();
}

} // namespace

void ConfigStack::AddLayer(ConfigLayer layer) {
    layers_.push_back(std::move(layer));
}

nlohmann::json ConfigStack::Resolve(bool include_user_overrides) const {
    nlohmann::json result = nlohmann::json::object();
    for (const auto& layer : layers_) {
        if (!include_user_overrides && layer.scope == ConfigScope::UserOverride) {
            continue;
        }
        MergeJsonObject(result, layer.values);
    }
    return result;
}

std::string ConfigStack::Hash(bool include_user_overrides) const {
    const auto text = Resolve(include_user_overrides).dump();
    return StableHash(text);
}

ResolvedConfig ConfigStack::ResolveForProfile(ConfigResolveProfile profile) const {
    nlohmann::json result = nlohmann::json::object();
    bool user_overrides_included = false;
    for (const auto& layer : layers_) {
        if (!IncludeScope(profile, layer.scope)) {
            continue;
        }
        if (layer.scope == ConfigScope::UserOverride) {
            user_overrides_included = true;
        }
        MergeJsonObject(result, layer.values);
    }
    const auto hash = StableHash(result.dump());
    return {profile, std::move(result), hash, user_overrides_included};
}

std::string ConfigStack::HashForProfile(ConfigResolveProfile profile) const {
    return ResolveForProfile(profile).hash;
}

void MergeJsonObject(nlohmann::json& target, const nlohmann::json& overlay) {
    for (auto it = overlay.begin(); it != overlay.end(); ++it) {
        if (it->is_object() && target.contains(it.key()) && target[it.key()].is_object()) {
            MergeJsonObject(target[it.key()], *it);
        } else {
            target[it.key()] = *it;
        }
    }
}

std::string ToString(ConfigScope scope) {
    switch (scope) {
    case ConfigScope::EngineDefault:
        return "engine_default";
    case ConfigScope::PlatformDefault:
        return "platform_default";
    case ConfigScope::Project:
        return "project";
    case ConfigScope::RuntimeProfile:
        return "runtime_profile";
    case ConfigScope::ReleaseProfile:
        return "release_profile";
    case ConfigScope::UserOverride:
        return "user_override";
    case ConfigScope::CommandLine:
        return "command_line";
    }
    return "engine_default";
}

std::string ToString(ConfigResolveProfile profile) {
    switch (profile) {
    case ConfigResolveProfile::Development:
        return "development";
    case ConfigResolveProfile::Runtime:
        return "runtime";
    case ConfigResolveProfile::Release:
        return "release";
    }
    return "development";
}

nlohmann::json ToJson(const ResolvedConfig& config) {
    return {
        {"profile", ToString(config.profile)},
        {"values", config.values},
        {"hash", config.hash},
        {"user_overrides_included", config.user_overrides_included},
    };
}

} // namespace Astra::Core
