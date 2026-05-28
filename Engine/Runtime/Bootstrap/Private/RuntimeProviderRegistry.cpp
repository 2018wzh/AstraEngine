#include <Astra/Bootstrap/RuntimeProviderRegistry.h>

#include <algorithm>

namespace astra {

template <typename Provider>
VoidResult RuntimeProviderRegistry::register_provider(
    std::unordered_map<std::string, std::shared_ptr<Provider>>& map,
    std::shared_ptr<Provider> provider, std::string_view kind, DiagnosticSink& diagnostics) {
    if (provider == nullptr) {
        diagnostics.error("provider.null", std::string(kind) + " provider is null");
        return std::unexpected(make_error("provider.null", "Provider is null"));
    }
    const std::string id = provider->id();
    if (id.empty()) {
        diagnostics.error("provider.empty_id", std::string(kind) + " provider id is empty");
        return std::unexpected(make_error("provider.empty_id", "Provider id is empty"));
    }
    if (map.contains(id)) {
        diagnostics.error("provider.duplicate",
                          "Duplicate " + std::string(kind) + " provider: " + id);
        return std::unexpected(make_error("provider.duplicate", "Duplicate provider"));
    }
    map.emplace(id, std::move(provider));
    return {};
}

template <typename Provider>
Expected<std::shared_ptr<Provider>> RuntimeProviderRegistry::find_provider(
    const std::unordered_map<std::string, std::shared_ptr<Provider>>& map, std::string_view kind,
    std::string_view id) const {
    if (map.empty()) {
        return std::unexpected(
            make_error("provider.missing", "No " + std::string(kind) + " provider registered"));
    }
    if (id.empty()) {
        return map.begin()->second;
    }
    const auto it = map.find(std::string(id));
    if (it == map.end()) {
        return std::unexpected(make_error(
            "provider.not_found", std::string(kind) + " provider not found: " + std::string(id)));
    }
    return it->second;
}

VoidResult
RuntimeProviderRegistry::register_platform_provider(std::shared_ptr<IPlatformProvider> provider,
                                                    DiagnosticSink& diagnostics) {
    return register_provider(platform_providers_, std::move(provider), "platform", diagnostics);
}

VoidResult
RuntimeProviderRegistry::register_renderer_provider(std::shared_ptr<IRendererProvider> provider,
                                                    DiagnosticSink& diagnostics) {
    return register_provider(renderer_providers_, std::move(provider), "renderer", diagnostics);
}

VoidResult
RuntimeProviderRegistry::register_audio_provider(std::shared_ptr<IAudioProvider> provider,
                                                 DiagnosticSink& diagnostics) {
    return register_provider(audio_providers_, std::move(provider), "audio", diagnostics);
}

VoidResult RuntimeProviderRegistry::register_project_content_provider(
    std::shared_ptr<IProjectContentProvider> provider, DiagnosticSink& diagnostics) {
    return register_provider(project_content_providers_, std::move(provider), "project_content",
                             diagnostics);
}

Expected<std::shared_ptr<IPlatformProvider>>
RuntimeProviderRegistry::platform_provider(std::string_view id) const {
    return find_provider(platform_providers_, "platform", id);
}

Expected<std::shared_ptr<IRendererProvider>>
RuntimeProviderRegistry::renderer_provider(std::string_view id) const {
    return find_provider(renderer_providers_, "renderer", id);
}

Expected<std::shared_ptr<IAudioProvider>>
RuntimeProviderRegistry::audio_provider(std::string_view id) const {
    return find_provider(audio_providers_, "audio", id);
}

Expected<std::shared_ptr<IProjectContentProvider>>
RuntimeProviderRegistry::project_content_provider(std::string_view id) const {
    return find_provider(project_content_providers_, "project_content", id);
}

std::vector<std::string> RuntimeProviderRegistry::provider_ids() const {
    std::vector<std::string> ids;
    for (const auto& [id, _] : platform_providers_) {
        ids.push_back("platform:" + id);
    }
    for (const auto& [id, _] : renderer_providers_) {
        ids.push_back("renderer:" + id);
    }
    for (const auto& [id, _] : audio_providers_) {
        ids.push_back("audio:" + id);
    }
    for (const auto& [id, _] : project_content_providers_) {
        ids.push_back("project_content:" + id);
    }
    std::sort(ids.begin(), ids.end());
    return ids;
}

void RuntimeProviderRegistry::clear() {
    project_content_providers_.clear();
    audio_providers_.clear();
    renderer_providers_.clear();
    platform_providers_.clear();
}

} // namespace astra
