#pragma once

#include <string>
#include <vector>

namespace Astra::Core {

struct BuildInfo {
    std::string engine_version;
    std::string git_commit;
    std::string build_config;
    unsigned abi_version = 0;
    bool runtime_enabled = false;
    bool tools_enabled = false;
    bool tests_enabled = false;
    bool samples_enabled = false;
    bool plugins_enabled = false;
    bool editor_enabled = false;
    bool headless_backend_enabled = false;
    bool sdl_backend_enabled = false;

    [[nodiscard]] std::vector<std::string> EnabledFeatures() const;
};

BuildInfo GetBuildInfo();

} // namespace Astra::Core
