#include <Astra/PropertySystem/PropertySystem.hpp>

#include <algorithm>

namespace Astra::PropertySystem {

PropertyFlag operator|(PropertyFlag lhs, PropertyFlag rhs) {
    return static_cast<PropertyFlag>(static_cast<Astra::Core::u32>(lhs) | static_cast<Astra::Core::u32>(rhs));
}

bool HasFlag(PropertyFlag value, PropertyFlag flag) {
    return (static_cast<Astra::Core::u32>(value) & static_cast<Astra::Core::u32>(flag)) != 0;
}

namespace {

std::string JsonTypeFor(const PropertyDescriptor& property) {
    if (property.kind == TypeKind::Array) {
        return "array";
    }
    if (property.kind == TypeKind::Map || property.kind == TypeKind::Struct || property.kind == TypeKind::TaggedUnion) {
        return "object";
    }
    if (property.kind == TypeKind::LocalizedText || property.kind == TypeKind::AssetRef || property.kind == TypeKind::Enum) {
        return "string";
    }
    if (property.type == "number" || property.type == "double" || property.type == "float") {
        return "number";
    }
    if (property.type == "integer" || property.type == "i32" || property.type == "u32") {
        return "integer";
    }
    if (property.type == "bool" || property.type == "boolean") {
        return "boolean";
    }
    return "string";
}

nlohmann::json SchemaForProperty(const PropertyDescriptor& property, const TypeRegistry* registry, Astra::Core::DiagnosticSink* diagnostics) {
    nlohmann::json schema = {
        {"type", JsonTypeFor(property)},
        {"x-astra-kind", static_cast<int>(property.kind)},
        {"x-astra-flags", static_cast<Astra::Core::u32>(property.flags)},
        {"x-astra-inspector", {
            {"display_name", property.inspector.display_name},
            {"category", property.inspector.category},
            {"tooltip", property.inspector.tooltip},
            {"order", property.inspector.order},
            {"visibility_condition", property.inspector.visibility_condition},
        }},
    };
    if (registry != nullptr && (property.kind == TypeKind::Struct || property.kind == TypeKind::TaggedUnion)) {
        const auto nested = registry->GenerateJsonSchema(property.type, *diagnostics);
        if (!nested.empty()) {
            schema = nested;
            schema["x-astra-kind"] = static_cast<int>(property.kind);
            schema["x-astra-flags"] = static_cast<Astra::Core::u32>(property.flags);
        }
    } else if (registry != nullptr && property.kind == TypeKind::Array) {
        const auto nested = registry->GenerateJsonSchema(property.type, *diagnostics);
        schema["items"] = nested.empty() ? nlohmann::json{{"type", "object"}} : nested;
    } else if (registry != nullptr && property.kind == TypeKind::Map) {
        const auto nested = registry->GenerateJsonSchema(property.type, *diagnostics);
        schema["additionalProperties"] = nested.empty() ? nlohmann::json{{"type", "object"}} : nested;
    }
    if (!property.default_value.is_null()) {
        schema["default"] = property.default_value;
    }
    if (property.validation.minimum) {
        schema["minimum"] = *property.validation.minimum;
    }
    if (property.validation.maximum) {
        schema["maximum"] = *property.validation.maximum;
    }
    if (!property.validation.regex.empty()) {
        schema["pattern"] = property.validation.regex;
    }
    if (!property.validation.dependencies.empty()) {
        schema["x-astra-dependencies"] = property.validation.dependencies;
    }
    if (!property.validation.custom_validator.empty()) {
        schema["x-astra-custom-validator"] = property.validation.custom_validator;
    }
    return schema;
}

} // namespace

Astra::Core::Result<void> TypeRegistry::Register(TypeDescriptor descriptor) {
    if (descriptor.type_id.empty()) {
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidArgument, "type descriptor requires type_id");
    }
    if (Find(descriptor.type_id) != nullptr) {
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidArgument, "type descriptor already registered");
    }
    types_.push_back(std::move(descriptor));
    return Astra::Core::Result<void>::Success();
}

Astra::Core::Result<void> TypeRegistry::RegisterMigration(SchemaVersionEdge edge) {
    if (edge.type_id.empty() || edge.from_version >= edge.to_version) {
        return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidArgument, "schema migration edge requires type and forward versions");
    }
    migrations_.push_back(std::move(edge));
    return Astra::Core::Result<void>::Success();
}

const TypeDescriptor* TypeRegistry::Find(std::string_view type_id) const {
    auto it = std::ranges::find_if(types_, [&](const TypeDescriptor& descriptor) { return descriptor.type_id == type_id; });
    return it == types_.end() ? nullptr : &*it;
}

void TypeRegistry::RegisterValidator(std::string id, CustomValidator validator) {
    validators_[std::move(id)] = std::move(validator);
}

nlohmann::json TypeRegistry::GenerateJsonSchema(std::string_view type_id, Astra::Core::DiagnosticSink& diagnostics) const {
    const auto* descriptor = Find(type_id);
    if (descriptor == nullptr) {
        Astra::Core::Diagnostic diagnostic;
        diagnostic.code = "ASTRA_PROPERTY_TYPE_MISSING";
        diagnostic.category = "property.schema";
        diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
        diagnostic.message = "Property type is not registered.";
        diagnostics.Emit(std::move(diagnostic));
        return nlohmann::json::object();
    }
    nlohmann::json properties = nlohmann::json::object();
    nlohmann::json required = nlohmann::json::array();
    for (const auto& property : descriptor->properties) {
        properties[property.id] = SchemaForProperty(property, this, &diagnostics);
        if (property.validation.required) {
            required.push_back(property.id);
        }
    }
    auto schema = ToJsonSchema(*descriptor);
    schema["properties"] = std::move(properties);
    if (!required.empty()) {
        schema["required"] = required;
    }
    return schema;
}

Astra::Core::Result<nlohmann::json> TypeRegistry::Validate(std::string_view type_id, const nlohmann::json& value, Astra::Core::DiagnosticSink& diagnostics) const {
    const auto* descriptor = Find(type_id);
    if (descriptor == nullptr) {
        return Astra::Core::Result<nlohmann::json>::Failure(Astra::Core::ErrorCode::NotFound, "type not registered");
    }

    nlohmann::json result = value;
    for (const auto& property : descriptor->properties) {
        if (!result.contains(property.id)) {
            if (property.validation.required) {
                Astra::Core::Diagnostic diagnostic;
                diagnostic.code = "ASTRA_PROPERTY_REQUIRED";
                diagnostic.category = "property.validation";
                diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
                diagnostic.message = "Required property is missing.";
                diagnostic.objects = {{"PropertyId", property.id}};
                diagnostics.Emit(std::move(diagnostic));
            } else if (!property.default_value.is_null()) {
                result[property.id] = property.default_value;
            }
        }
        for (const auto& dependency : property.validation.dependencies) {
            if (result.contains(property.id) && !result.contains(dependency)) {
                Astra::Core::Diagnostic diagnostic;
                diagnostic.code = "ASTRA_PROPERTY_DEPENDENCY";
                diagnostic.category = "property.validation";
                diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
                diagnostic.message = "Property dependency is missing.";
                diagnostic.objects = {{"PropertyId", property.id}, {"DependsOn", dependency}};
                diagnostics.Emit(std::move(diagnostic));
            }
        }
        if (!property.validation.custom_validator.empty() && result.contains(property.id)) {
            auto validator = validators_.find(property.validation.custom_validator);
            if (validator == validators_.end()) {
                Astra::Core::Diagnostic diagnostic;
                diagnostic.code = "ASTRA_PROPERTY_VALIDATOR_MISSING";
                diagnostic.category = "property.validation";
                diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
                diagnostic.message = "Custom property validator is not registered.";
                diagnostic.objects = {{"PropertyId", property.id}, {"Validator", property.validation.custom_validator}};
                diagnostics.Emit(std::move(diagnostic));
            } else if (auto custom_result = validator->second(result[property.id]); !custom_result) {
                Astra::Core::Diagnostic diagnostic;
                diagnostic.code = "ASTRA_PROPERTY_CUSTOM_VALIDATION";
                diagnostic.category = "property.validation";
                diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
                diagnostic.message = custom_result.Message();
                diagnostic.objects = {{"PropertyId", property.id}, {"Validator", property.validation.custom_validator}};
                diagnostics.Emit(std::move(diagnostic));
            }
        }
    }

    if (diagnostics.HasBlocking()) {
        return Astra::Core::Result<nlohmann::json>::Failure(Astra::Core::ErrorCode::InvalidFormat, "property validation failed");
    }
    return Astra::Core::Result<nlohmann::json>::Success(result);
}

Astra::Core::Result<void> TypeRegistry::ValidateSchemaVersion(std::string_view type_id, Astra::Core::u32 from_version, Astra::Core::u32 to_version, Astra::Core::DiagnosticSink& diagnostics) const {
    if (from_version == to_version) {
        return Astra::Core::Result<void>::Success();
    }
    Astra::Core::u32 current = from_version;
    while (current < to_version) {
        auto edge = std::ranges::find_if(migrations_, [&](const SchemaVersionEdge& item) {
            return item.type_id == type_id && item.from_version == current && item.to_version == current + 1;
        });
        if (edge == migrations_.end()) {
            Astra::Core::Diagnostic diagnostic;
            diagnostic.code = "ASTRA_PROPERTY_MIGRATION_MISSING";
            diagnostic.category = "property.migration";
            diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
            diagnostic.message = "Property schema migration path is missing.";
            diagnostic.objects = {{"TypeId", std::string(type_id)}};
            diagnostics.Emit(std::move(diagnostic));
            return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::NotFound, "property migration path missing");
        }
        current = edge->to_version;
    }
    return Astra::Core::Result<void>::Success();
}

PropertyWriteResult TypeRegistry::EvaluateWrite(const PropertyWriteRequest& request) const {
    PropertyWriteResult result;
    const auto* descriptor = Find(request.type_id);
    if (descriptor == nullptr) {
        result.allowed = false;
        Astra::Core::Diagnostic diagnostic;
        diagnostic.code = "ASTRA_PROPERTY_TYPE_MISSING";
        diagnostic.category = "property.write";
        diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
        diagnostic.message = "Property type is not registered.";
        result.diagnostics.push_back(std::move(diagnostic));
        return result;
    }
    auto property = std::ranges::find_if(descriptor->properties, [&](const PropertyDescriptor& item) { return item.id == request.property_id; });
    if (property == descriptor->properties.end()) {
        result.allowed = false;
        Astra::Core::Diagnostic diagnostic;
        diagnostic.code = "ASTRA_PROPERTY_WRITE_UNKNOWN";
        diagnostic.category = "property.write";
        diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
        diagnostic.message = "Property write references an unknown property.";
        result.diagnostics.push_back(std::move(diagnostic));
        return result;
    }

    result.requires_review = HasFlag(property->flags, PropertyFlag::RequiresReview);
    result.release_sensitive = HasFlag(property->flags, PropertyFlag::ReleaseSensitive);
    auto reject = [&](std::string code, std::string message) {
        result.allowed = false;
        Astra::Core::Diagnostic diagnostic;
        diagnostic.code = std::move(code);
        diagnostic.category = "property.write";
        diagnostic.severity = Astra::Core::DiagnosticSeverity::Blocking;
        diagnostic.message = std::move(message);
        diagnostic.objects = {{"PropertyId", property->id}, {"TypeId", descriptor->type_id}};
        result.diagnostics.push_back(std::move(diagnostic));
    };
    if (HasFlag(property->flags, PropertyFlag::ReadOnly)) {
        reject("ASTRA_PROPERTY_WRITE_READ_ONLY", "Read-only property cannot be written.");
    }
    if (request.ai_writer && !HasFlag(property->flags, PropertyFlag::AiEditable)) {
        reject("ASTRA_PROPERTY_WRITE_AI_DENIED", "AI writer cannot edit a property without ai_editable flag.");
    }
    if (request.runtime_writer && HasFlag(property->flags, PropertyFlag::EditorOnly)) {
        reject("ASTRA_PROPERTY_WRITE_RUNTIME_DENIED", "Runtime writer cannot edit editor-only property.");
    }
    if (request.editor_writer && HasFlag(property->flags, PropertyFlag::RuntimeOnly)) {
        reject("ASTRA_PROPERTY_WRITE_EDITOR_DENIED", "Editor writer cannot edit runtime-only property.");
    }
    if (request.release_mode && result.release_sensitive) {
        reject("ASTRA_PROPERTY_WRITE_RELEASE_SENSITIVE", "Release-sensitive property write is blocked in release mode.");
    }
    return result;
}

nlohmann::json TypeRegistry::ApplyMigration(const nlohmann::json& value, const std::vector<MigrationStep>& steps) const {
    nlohmann::json result = value;
    for (const auto& step : steps) {
        if (!step.from_property.empty() && !step.to_property.empty() && result.contains(step.from_property)) {
            result[step.to_property] = result[step.from_property];
            result.erase(step.from_property);
        }
        if (!step.to_property.empty() && !step.default_value.is_null() && !result.contains(step.to_property)) {
            result[step.to_property] = step.default_value;
        }
        if (step.deprecated && !step.from_property.empty()) {
            result.erase(step.from_property);
        }
    }
    return result;
}

std::vector<PropertyDiff> TypeRegistry::Diff(std::string_view type_id, const nlohmann::json& before, const nlohmann::json& after) const {
    std::vector<PropertyDiff> diffs;
    const auto* descriptor = Find(type_id);
    if (descriptor == nullptr) {
        return diffs;
    }

    for (const auto& property : descriptor->properties) {
        if (!property.include_in_diff) {
            continue;
        }
        const auto before_value = before.contains(property.id) ? before.at(property.id) : nlohmann::json();
        const auto after_value = after.contains(property.id) ? after.at(property.id) : nlohmann::json();
        if (before_value != after_value) {
            diffs.push_back({property.id, before_value, after_value, HasFlag(property.flags, PropertyFlag::RequiresReview), HasFlag(property.flags, PropertyFlag::ReleaseSensitive)});
        }
    }
    return diffs;
}

nlohmann::json ToJsonSchema(const TypeDescriptor& descriptor) {
    nlohmann::json properties = nlohmann::json::object();
    nlohmann::json required = nlohmann::json::array();

    for (const auto& property : descriptor.properties) {
        nlohmann::json schema = SchemaForProperty(property, nullptr, nullptr);
        if (property.validation.required) {
            required.push_back(property.id);
        }
        properties[property.id] = schema;
    }

    nlohmann::json schema = {
        {"$schema", "https://json-schema.org/draft/2020-12/schema"},
        {"$id", descriptor.type_id},
        {"type", "object"},
        {"x-astra-version", descriptor.version},
        {"properties", properties},
        {"additionalProperties", true},
    };
    if (!required.empty()) {
        schema["required"] = required;
    }
    return schema;
}

nlohmann::json ToJson(const PropertyDiff& diff) {
    return {
        {"property_id", diff.property_id},
        {"before", diff.before},
        {"after", diff.after},
        {"requires_review", diff.requires_review},
        {"release_sensitive", diff.release_sensitive},
    };
}

nlohmann::json ToJson(const PropertyWriteResult& result) {
    nlohmann::json diagnostics = nlohmann::json::array();
    for (const auto& diagnostic : result.diagnostics) {
        diagnostics.push_back(Astra::Core::ToJson(diagnostic));
    }
    return {
        {"allowed", result.allowed},
        {"requires_review", result.requires_review},
        {"release_sensitive", result.release_sensitive},
        {"diagnostics", diagnostics},
    };
}

} // namespace Astra::PropertySystem
