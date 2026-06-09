#include <Astra/Core/BuildInfo.hpp>

namespace Astra::Core {

std::vector<std::string> BuildInfo::EnabledFeatures() const {
    std::vector<std::string> features;
    if (runtime_enabled) {
        features.emplace_back("runtime");
    }
    if (tools_enabled) {
        features.emplace_back("tools");
    }
    if (tests_enabled) {
        features.emplace_back("tests");
    }
    if (samples_enabled) {
        features.emplace_back("samples");
    }
    if (plugins_enabled) {
        features.emplace_back("plugins");
    }
    if (editor_enabled) {
        features.emplace_back("editor");
    }
    if (headless_backend_enabled) {
        features.emplace_back("platform.headless");
    }
    if (sdl_backend_enabled) {
        features.emplace_back("platform.sdl");
    }
    return features;
}

} // namespace Astra::Core

