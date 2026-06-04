#include <Astra/PropertySystem/PropertySystem.h>

namespace astra {

namespace {

nlohmann::json property_schema(const PropertyDescriptor& descriptor) {
    nlohmann::json schema = nlohmann::json::object();
    switch (descriptor.kind) {
    case PropertyTypeKind::Boolean:
        schema["type"] = "boolean";
        break;
    case PropertyTypeKind::Integer:
        schema["type"] = "integer";
        break;
    case PropertyTypeKind::Number:
        schema["type"] = "number";
        break;
    case PropertyTypeKind::String:
        schema["type"] = "string";
        break;
    case PropertyTypeKind::Enum:
        schema["type"] = "string";
        schema["enum"] = descriptor.enum_values;
        break;
    case PropertyTypeKind::Array:
        schema["type"] = "array";
        if (!descriptor.value_type.empty()) {
            schema["items"] = {{"$ref", descriptor.value_type}};
        }
        break;
    case PropertyTypeKind::Struct:
        schema["type"] = "object";
        if (!descriptor.value_type.empty()) {
            schema["$ref"] = descriptor.value_type;
        }
        break;
    }
    if (!descriptor.default_value.is_null()) {
        schema["default"] = descriptor.default_value;
    }
    schema["x-astra-flags"] = {
        {"ai_editable", has_flag(descriptor.flags, PropertyFlags::AiEditable)},
        {"tool_generated", has_flag(descriptor.flags, PropertyFlags::ToolGenerated)},
        {"read_only", has_flag(descriptor.flags, PropertyFlags::ReadOnly)},
        {"requires_review", has_flag(descriptor.flags, PropertyFlags::RequiresReview)},
    };
    return schema;
}

} // namespace

VoidResult PropertyRegistry::register_type(TypeDescriptor descriptor, DiagnosticSink& diagnostics) {
    if (descriptor.id.empty()) {
        diagnostics.error("property.empty_type_id", "Type id must not be empty");
        return std::unexpected(make_error("property.empty_type_id", "Type id must not be empty"));
    }
    if (types_.contains(descriptor.id)) {
        diagnostics.error("property.duplicate_type", "Duplicate type id: " + descriptor.id);
        return std::unexpected(make_error("property.duplicate_type", "Duplicate type id"));
    }
    for (const PropertyDescriptor& property : descriptor.properties) {
        if (property.id.empty()) {
            diagnostics.error("property.empty_property_id",
                              "Property id must not be empty in type " + descriptor.id);
            return std::unexpected(
                make_error("property.empty_property_id", "Property id must not be empty"));
        }
    }
    types_.emplace(descriptor.id, std::move(descriptor));
    return {};
}

bool PropertyRegistry::contains(std::string_view type_id) const {
    return types_.contains(std::string(type_id));
}

const TypeDescriptor* PropertyRegistry::find(std::string_view type_id) const {
    const auto it = types_.find(std::string(type_id));
    if (it == types_.end()) {
        return nullptr;
    }
    return &it->second;
}

nlohmann::json PropertyRegistry::generate_json_schema(std::string_view type_id) const {
    const TypeDescriptor* descriptor = find(type_id);
    if (descriptor == nullptr) {
        return nlohmann::json::object();
    }
    nlohmann::json schema = {
        {"$id", descriptor->id},
        {"title", descriptor->display_name},
        {"type", "object"},
        {"properties", nlohmann::json::object()},
        {"additionalProperties", false},
    };
    for (const PropertyDescriptor& property : descriptor->properties) {
        schema["properties"][property.id] = property_schema(property);
    }
    return schema;
}

} // namespace astra
