#include <Astra/ModuleRuntime/PluginDescriptor.h>

#include <Astra/Core/Config.h>
#include <Astra/Core/Path.h>

#include <yaml-cpp/yaml.h>

#include <algorithm>
#include <stdexcept>

namespace astra {

namespace {

std::string required_string(const YAML::Node& node, const char* field) {
    if (!node[field] || !node[field].IsScalar()) {
        throw std::runtime_error(std::string("missing required field: ") + field);
    }
    return node[field].as<std::string>();
}

ModuleType parse_module_type(std::string_view value) {
    if (value == "runtime") {
        return ModuleType::Runtime;
    }
    if (value == "editor") {
        return ModuleType::Editor;
    }
    if (value == "developer") {
        return ModuleType::Developer;
    }
    throw std::runtime_error("unsupported module type: " + std::string(value));
}

LoadPhase parse_load_phase(std::string_view value) {
    if (value == "core_startup") {
        return LoadPhase::CoreStartup;
    }
    if (value == "project_load") {
        return LoadPhase::ProjectLoad;
    }
    if (value == "runtime_startup") {
        return LoadPhase::RuntimeStartup;
    }
    if (value == "editor_startup") {
        return LoadPhase::EditorStartup;
    }
    throw std::runtime_error("unsupported load phase: " + std::string(value));
}

void collect_permissions(const YAML::Node& node, const std::string& prefix, PermissionSet& out) {
    if (!node) {
        return;
    }
    if (node.IsScalar()) {
        if (node.as<bool>()) {
            out.insert(prefix);
        }
        return;
    }
    if (node.IsMap()) {
        for (const auto& item : node) {
            const std::string key = item.first.as<std::string>();
            const std::string next = prefix.empty() ? key : prefix + "." + key;
            collect_permissions(item.second, next, out);
        }
    }
}

std::vector<std::string> string_sequence(const YAML::Node& node) {
    std::vector<std::string> result;
    if (!node) {
        return result;
    }
    if (!node.IsSequence()) {
        throw std::runtime_error("expected sequence");
    }
    for (const auto& item : node) {
        result.push_back(item.as<std::string>());
    }
    return result;
}

nlohmann::json plugin_descriptor_schema() {
    return nlohmann::json::parse(R"JSON(
{
  "type": "object",
  "required": ["id", "display_name", "version", "astra_api", "modules"],
  "additionalProperties": false,
  "properties": {
    "id": { "type": "string", "pattern": "^astra\\.plugin\\.[a-z0-9_.-]+$" },
    "display_name": { "type": "string" },
    "version": { "type": "string" },
    "astra_api": { "type": "string" },
    "dependencies": { "type": "array", "items": { "type": "string" } },
    "modules": {
      "type": "array",
      "minItems": 1,
      "items": {
        "type": "object",
        "required": ["id", "type", "entrypoint", "load_phase", "capabilities"],
        "additionalProperties": false,
        "properties": {
          "id": { "type": "string" },
          "type": { "type": "string", "enum": ["runtime", "editor", "developer"] },
          "entrypoint": { "type": "string" },
          "load_phase": {
            "type": "string",
            "enum": ["core_startup", "project_load", "runtime_startup", "editor_startup"]
          },
          "capabilities": { "type": "array", "items": { "type": "string" } },
          "permissions": { "type": "object" },
          "platforms": { "type": "array", "items": { "type": "string" } }
        }
      }
    }
  }
}
)JSON");
}

} // namespace

Expected<PluginDescriptor> load_plugin_descriptor(const std::filesystem::path& path,
                                                  DiagnosticSink& diagnostics) {
    try {
        auto descriptor_json = load_yaml_file_as_json(path, diagnostics);
        if (!descriptor_json) {
            return std::unexpected(descriptor_json.error());
        }
        DiagnosticSink schema_diagnostics;
        if (auto valid = validate_json_schema(*descriptor_json, plugin_descriptor_schema(),
                                              schema_diagnostics, path_to_utf8(path));
            !valid) {
            for (const Diagnostic& diagnostic : schema_diagnostics.diagnostics()) {
                diagnostics.emit(diagnostic);
            }
            return std::unexpected(valid.error());
        }

        const YAML::Node root = YAML::LoadFile(path_to_utf8(path));
        PluginDescriptor descriptor;
        descriptor.descriptor_path = path;
        descriptor.id = required_string(root, "id");
        descriptor.display_name = required_string(root, "display_name");
        descriptor.version = required_string(root, "version");
        descriptor.astra_api = required_string(root, "astra_api");
        descriptor.dependencies = string_sequence(root["dependencies"]);

        for (const auto& module_node : root["modules"]) {
            ModuleDescriptor module;
            module.id = required_string(module_node, "id");
            module.type = parse_module_type(required_string(module_node, "type"));
            module.entrypoint = path.parent_path() / required_string(module_node, "entrypoint");
            module.load_phase = parse_load_phase(required_string(module_node, "load_phase"));
            for (const std::string& capability : string_sequence(module_node["capabilities"])) {
                module.capabilities.insert(capability);
            }
            collect_permissions(module_node["permissions"], "", module.permissions);
            module.platforms = string_sequence(module_node["platforms"]);
            descriptor.modules.push_back(std::move(module));
        }

        return descriptor;
    } catch (const std::exception& ex) {
        diagnostics.error("plugin_descriptor.parse", path_to_utf8(path) + ": " + ex.what());
        return std::unexpected(make_error("plugin_descriptor.parse", ex.what()));
    }
}

std::string module_type_to_string(ModuleType type) {
    switch (type) {
    case ModuleType::Runtime:
        return "runtime";
    case ModuleType::Editor:
        return "editor";
    case ModuleType::Developer:
        return "developer";
    }
    return "runtime";
}

std::string load_phase_to_string(LoadPhase phase) {
    switch (phase) {
    case LoadPhase::CoreStartup:
        return "core_startup";
    case LoadPhase::ProjectLoad:
        return "project_load";
    case LoadPhase::RuntimeStartup:
        return "runtime_startup";
    case LoadPhase::EditorStartup:
        return "editor_startup";
    }
    return "runtime_startup";
}

int load_phase_order(LoadPhase phase) {
    switch (phase) {
    case LoadPhase::CoreStartup:
        return 0;
    case LoadPhase::ProjectLoad:
        return 10;
    case LoadPhase::RuntimeStartup:
        return 20;
    case LoadPhase::EditorStartup:
        return 30;
    }
    return 20;
}

} // namespace astra
