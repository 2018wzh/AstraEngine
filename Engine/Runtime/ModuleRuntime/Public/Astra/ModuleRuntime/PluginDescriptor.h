#pragma once

#include <Astra/Core/Diagnostics.h>
#include <Astra/Core/Result.h>

#include <filesystem>
#include <string>
#include <unordered_set>
#include <vector>

namespace astra {

enum class ModuleType {
    Runtime,
    Editor,
    Developer,
};

enum class LoadPhase {
    CoreStartup,
    ProjectLoad,
    RuntimeStartup,
    EditorStartup,
};

using CapabilitySet = std::unordered_set<std::string>;
using PermissionSet = std::unordered_set<std::string>;

struct ModuleDescriptor {
    std::string id;
    ModuleType type = ModuleType::Runtime;
    std::filesystem::path entrypoint;
    LoadPhase load_phase = LoadPhase::RuntimeStartup;
    CapabilitySet capabilities;
    PermissionSet permissions;
    std::vector<std::string> platforms;
};

struct PluginDescriptor {
    std::filesystem::path descriptor_path;
    std::string id;
    std::string display_name;
    std::string version;
    std::string astra_api;
    std::vector<ModuleDescriptor> modules;
    std::vector<std::string> dependencies;
};

Expected<PluginDescriptor> load_plugin_descriptor(const std::filesystem::path& path,
                                                  DiagnosticSink& diagnostics);

std::string module_type_to_string(ModuleType type);
std::string load_phase_to_string(LoadPhase phase);
int load_phase_order(LoadPhase phase);

} // namespace astra
