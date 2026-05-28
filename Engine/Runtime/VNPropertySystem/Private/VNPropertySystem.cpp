#include <Astra/VNPropertySystem/VNPropertySystem.h>

namespace astra {

namespace {

std::string json_type_for(VNPropertyKind kind) {
    switch (kind) {
    case VNPropertyKind::Boolean:
        return "boolean";
    case VNPropertyKind::Integer:
        return "integer";
    case VNPropertyKind::Number:
        return "number";
    case VNPropertyKind::String:
    case VNPropertyKind::LocalizedText:
    case VNPropertyKind::AssetRef:
    case VNPropertyKind::Enum:
        return "string";
    }
    return "string";
}

} // namespace

VoidResult VNPropertyRegistry::register_type(VNTypeDescriptor descriptor,
                                             DiagnosticSink& diagnostics) {
    if (descriptor.type_id.empty()) {
        diagnostics.error("vn_property.empty_type_id", "VN property type_id must not be empty");
        return std::unexpected(make_error("vn_property.empty_type_id", "Empty type id"));
    }
    if (types_.contains(descriptor.type_id)) {
        diagnostics.error("vn_property.duplicate_type",
                          "Duplicate VN property type: " + descriptor.type_id);
        return std::unexpected(make_error("vn_property.duplicate_type", "Duplicate type"));
    }
    types_.emplace(descriptor.type_id, std::move(descriptor));
    return {};
}

nlohmann::json VNPropertyRegistry::generate_json_schema(std::string_view type_id) const {
    const auto it = types_.find(std::string{type_id});
    if (it == types_.end()) {
        return nlohmann::json{{"$schema", "https://json-schema.org/draft/2020-12/schema"},
                              {"type", "object"},
                              {"additionalProperties", false}};
    }

    nlohmann::json properties = nlohmann::json::object();
    std::vector<std::string> required;
    for (const VNPropertyDescriptor& property : it->second.properties) {
        nlohmann::json property_schema{{"type", json_type_for(property.kind)}};
        if (!property.default_value.is_null()) {
            property_schema["default"] = property.default_value;
        }
        if (property.kind == VNPropertyKind::AssetRef) {
            property_schema["pattern"] = "^[a-zA-Z0-9_-]+:/.*$";
        }
        if (property.kind == VNPropertyKind::Enum) {
            property_schema["enum"] = property.enum_values;
        }
        property_schema["x-astra-ai-editable"] = property.ai_editable;
        property_schema["x-astra-tool-generated"] = property.tool_generated;
        property_schema["x-astra-read-only"] = property.read_only;
        property_schema["x-astra-requires-review"] = property.requires_review;
        properties[property.id] = std::move(property_schema);
        required.push_back(property.id);
    }

    return nlohmann::json{{"$schema", "https://json-schema.org/draft/2020-12/schema"},
                          {"title", it->second.display_name},
                          {"type", "object"},
                          {"additionalProperties", false},
                          {"properties", std::move(properties)},
                          {"required", std::move(required)}};
}

bool VNPropertyRegistry::contains(std::string_view type_id) const {
    return types_.contains(std::string{type_id});
}

} // namespace astra
