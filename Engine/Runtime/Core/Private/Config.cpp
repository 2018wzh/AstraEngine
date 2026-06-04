#include <Astra/Core/Config.h>

#include <Astra/Core/Path.h>

#include <yaml-cpp/yaml.h>

#include <fstream>
#include <regex>
#include <sstream>
#include <unordered_set>

namespace astra {

namespace {

nlohmann::json yaml_to_json(const YAML::Node& node) {
    if (!node) {
        return nullptr;
    }
    if (node.IsNull()) {
        return nullptr;
    }
    if (node.IsScalar()) {
        const std::string scalar = node.as<std::string>();
        if (scalar == "true") {
            return true;
        }
        if (scalar == "false") {
            return false;
        }
        try {
            std::size_t parsed = 0;
            const int integer = std::stoi(scalar, &parsed);
            if (parsed == scalar.size()) {
                return integer;
            }
        } catch (...) {
        }
        try {
            std::size_t parsed = 0;
            const double number = std::stod(scalar, &parsed);
            if (parsed == scalar.size()) {
                return number;
            }
        } catch (...) {
        }
        return scalar;
    }
    if (node.IsSequence()) {
        nlohmann::json array = nlohmann::json::array();
        for (const YAML::Node& item : node) {
            array.push_back(yaml_to_json(item));
        }
        return array;
    }
    if (node.IsMap()) {
        nlohmann::json object = nlohmann::json::object();
        for (const auto& item : node) {
            object[item.first.as<std::string>()] = yaml_to_json(item.second);
        }
        return object;
    }
    return nullptr;
}

bool matches_json_type(const nlohmann::json& value, std::string_view type) {
    if (type == "object") {
        return value.is_object();
    }
    if (type == "array") {
        return value.is_array();
    }
    if (type == "string") {
        return value.is_string();
    }
    if (type == "integer") {
        return value.is_number_integer();
    }
    if (type == "number") {
        return value.is_number();
    }
    if (type == "boolean") {
        return value.is_boolean();
    }
    if (type == "null") {
        return value.is_null();
    }
    return true;
}

std::string json_type_name(const nlohmann::json& value) {
    if (value.is_object()) {
        return "object";
    }
    if (value.is_array()) {
        return "array";
    }
    if (value.is_string()) {
        return "string";
    }
    if (value.is_number_integer()) {
        return "integer";
    }
    if (value.is_number()) {
        return "number";
    }
    if (value.is_boolean()) {
        return "boolean";
    }
    if (value.is_null()) {
        return "null";
    }
    return "unknown";
}

VoidResult validate_impl(const nlohmann::json& value, const nlohmann::json& schema,
                         DiagnosticSink& diagnostics, const std::string& path) {
    if (!schema.is_object()) {
        return {};
    }

    if (schema.contains("type")) {
        if (schema["type"].is_string()) {
            const std::string expected = schema["type"].get<std::string>();
            if (!matches_json_type(value, expected)) {
                diagnostics.error("schema.type_mismatch",
                                  path + " expected " + expected + ", got " +
                                      json_type_name(value));
            }
        } else if (schema["type"].is_array()) {
            bool matched = false;
            for (const auto& type : schema["type"]) {
                if (type.is_string() && matches_json_type(value, type.get<std::string>())) {
                    matched = true;
                }
            }
            if (!matched) {
                diagnostics.error("schema.type_mismatch",
                                  path + " did not match any allowed type");
            }
        }
    }

    if (schema.contains("enum") && schema["enum"].is_array()) {
        bool matched = false;
        for (const auto& item : schema["enum"]) {
            if (value == item) {
                matched = true;
                break;
            }
        }
        if (!matched) {
            diagnostics.error("schema.enum_mismatch", path + " is not an allowed value");
        }
    }

    if (schema.contains("pattern") && value.is_string() && schema["pattern"].is_string()) {
        try {
            const std::regex pattern(schema["pattern"].get<std::string>());
            if (!std::regex_match(value.get<std::string>(), pattern)) {
                diagnostics.error("schema.pattern_mismatch", path + " does not match pattern");
            }
        } catch (const std::exception& ex) {
            diagnostics.error("schema.invalid_pattern", path + ": " + ex.what());
        }
    }

    if (value.is_object()) {
        const nlohmann::json properties =
            schema.value("properties", nlohmann::json::object());

        if (schema.contains("required") && schema["required"].is_array()) {
            for (const auto& required : schema["required"]) {
                if (!required.is_string()) {
                    continue;
                }
                const std::string key = required.get<std::string>();
                if (!value.contains(key)) {
                    diagnostics.error("schema.required_missing", path + "." + key + " is required");
                }
            }
        }

        if (properties.is_object()) {
            for (const auto& [key, property_schema] : properties.items()) {
                if (value.contains(key)) {
                    if (auto result =
                            validate_impl(value.at(key), property_schema, diagnostics,
                                          path == "$" ? "$." + key : path + "." + key);
                        !result) {
                        return result;
                    }
                }
            }
        }

        if (schema.contains("additionalProperties") &&
            schema["additionalProperties"].is_boolean() &&
            !schema["additionalProperties"].get<bool>() && properties.is_object()) {
            std::unordered_set<std::string> known;
            for (const auto& [key, _] : properties.items()) {
                known.insert(key);
            }
            for (const auto& [key, _] : value.items()) {
                if (!known.contains(key)) {
                    diagnostics.error("schema.additional_property",
                                      (path == "$" ? "$." + key : path + "." + key) +
                                          " is not allowed");
                }
            }
        }
    }

    if (value.is_array()) {
        if (schema.contains("minItems") && schema["minItems"].is_number_unsigned() &&
            value.size() < schema["minItems"].get<std::size_t>()) {
            diagnostics.error("schema.min_items", path + " has too few items");
        }
        if (schema.contains("items")) {
            for (std::size_t i = 0; i < value.size(); ++i) {
                if (auto result = validate_impl(value[i], schema["items"], diagnostics,
                                                path + "[" + std::to_string(i) + "]");
                    !result) {
                    return result;
                }
            }
        }
    }

    return {};
}

} // namespace

Expected<nlohmann::json> load_yaml_file_as_json(const std::filesystem::path& path,
                                                DiagnosticSink& diagnostics) {
    try {
        return yaml_to_json(YAML::LoadFile(path_to_utf8(path)));
    } catch (const std::exception& ex) {
        diagnostics.error("config.yaml_parse", path_to_utf8(path) + ": " + ex.what());
        return std::unexpected(make_error("config.yaml_parse", ex.what()));
    }
}

Expected<nlohmann::json> load_json_file(const std::filesystem::path& path,
                                        DiagnosticSink& diagnostics) {
    std::ifstream input(path);
    if (!input) {
        diagnostics.error("config.json_open", "Cannot open JSON file: " + path_to_utf8(path));
        return std::unexpected(make_error("config.json_open", "Cannot open JSON file"));
    }
    try {
        nlohmann::json parsed;
        input >> parsed;
        return parsed;
    } catch (const std::exception& ex) {
        diagnostics.error("config.json_parse", path_to_utf8(path) + ": " + ex.what());
        return std::unexpected(make_error("config.json_parse", ex.what()));
    }
}

VoidResult validate_json_schema(const nlohmann::json& value, const nlohmann::json& schema,
                                DiagnosticSink& diagnostics, std::string path) {
    const bool had_errors = diagnostics.has_errors();
    if (auto result = validate_impl(value, schema, diagnostics, path); !result) {
        return result;
    }
    if (!had_errors && diagnostics.has_errors()) {
        return std::unexpected(make_error("schema.validation_failed", "Schema validation failed"));
    }
    return {};
}

VoidResult validate_yaml_file_with_schema(const std::filesystem::path& yaml_path,
                                          const std::filesystem::path& schema_path,
                                          DiagnosticSink& diagnostics) {
    auto value = load_yaml_file_as_json(yaml_path, diagnostics);
    if (!value) {
        return std::unexpected(value.error());
    }
    auto schema = load_json_file(schema_path, diagnostics);
    if (!schema) {
        return std::unexpected(schema.error());
    }
    return validate_json_schema(*value, *schema, diagnostics);
}

} // namespace astra
