#pragma once

#include <Astra/Core/Diagnostics.hpp>
#include <Astra/Core/StableId.hpp>
#include <nlohmann/json.hpp>

#include <optional>
#include <functional>
#include <map>
#include <string>
#include <vector>

namespace Astra::PropertySystem {

enum class TypeKind {
    Scalar,
    Enum,
    LocalizedText,
    AssetRef,
    Struct,
    Array,
    Map,
    TaggedUnion
};

enum class PropertyFlag : Astra::Core::u32 {
    None = 0,
    AiEditable = 1u << 0u,
    ToolGenerated = 1u << 1u,
    ReadOnly = 1u << 2u,
    RequiresReview = 1u << 3u,
    RuntimeOnly = 1u << 4u,
    EditorOnly = 1u << 5u,
    ReleaseSensitive = 1u << 6u
};

[[nodiscard]] PropertyFlag operator|(PropertyFlag lhs, PropertyFlag rhs);
[[nodiscard]] bool HasFlag(PropertyFlag value, PropertyFlag flag);

struct InspectorMetadata {
    std::string display_name;
    std::string category;
    std::string tooltip;
    Astra::Core::u32 order = 0;
    std::string visibility_condition;
};

struct ValidationRule {
    bool required = false;
    std::optional<double> minimum;
    std::optional<double> maximum;
    std::string regex;
    std::vector<std::string> dependencies;
    std::string custom_validator;
};

struct PropertyDescriptor {
    std::string id;
    std::string type;
    TypeKind kind = TypeKind::Scalar;
    PropertyFlag flags = PropertyFlag::None;
    nlohmann::json default_value;
    ValidationRule validation;
    InspectorMetadata inspector;
    std::string audit_label;
    bool include_in_diff = true;
};

struct TypeDescriptor {
    std::string type_id;
    TypeKind kind = TypeKind::Struct;
    Astra::Core::u32 version = 1;
    std::vector<PropertyDescriptor> properties;
    std::vector<std::string> enum_values;
};

struct MigrationStep {
    std::string from_property;
    std::string to_property;
    nlohmann::json default_value;
    bool deprecated = false;
};

struct PropertyDiff {
    std::string property_id;
    nlohmann::json before;
    nlohmann::json after;
    bool requires_review = false;
    bool release_sensitive = false;
};

struct SchemaVersionEdge {
    std::string type_id;
    Astra::Core::u32 from_version = 0;
    Astra::Core::u32 to_version = 0;
    std::vector<MigrationStep> steps;
};

struct PropertyWriteRequest {
    std::string type_id;
    std::string property_id;
    nlohmann::json before;
    nlohmann::json after;
    bool ai_writer = false;
    bool editor_writer = false;
    bool runtime_writer = false;
    bool release_mode = false;
};

struct PropertyWriteResult {
    bool allowed = true;
    bool requires_review = false;
    bool release_sensitive = false;
    std::vector<Astra::Core::Diagnostic> diagnostics;
};

class TypeRegistry {
public:
    using CustomValidator = std::function<Astra::Core::Result<void>(const nlohmann::json&)>;

    [[nodiscard]] Astra::Core::Result<void> Register(TypeDescriptor descriptor);
    [[nodiscard]] Astra::Core::Result<void> RegisterMigration(SchemaVersionEdge edge);
    void RegisterValidator(std::string id, CustomValidator validator);
    [[nodiscard]] const TypeDescriptor* Find(std::string_view type_id) const;
    [[nodiscard]] nlohmann::json GenerateJsonSchema(std::string_view type_id, Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] Astra::Core::Result<nlohmann::json> Validate(std::string_view type_id, const nlohmann::json& value, Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] Astra::Core::Result<void> ValidateSchemaVersion(std::string_view type_id, Astra::Core::u32 from_version, Astra::Core::u32 to_version, Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] PropertyWriteResult EvaluateWrite(const PropertyWriteRequest& request) const;
    [[nodiscard]] nlohmann::json ApplyMigration(const nlohmann::json& value, const std::vector<MigrationStep>& steps) const;
    [[nodiscard]] std::vector<PropertyDiff> Diff(std::string_view type_id, const nlohmann::json& before, const nlohmann::json& after) const;

private:
    std::vector<TypeDescriptor> types_;
    std::vector<SchemaVersionEdge> migrations_;
    std::map<std::string, CustomValidator> validators_;
};

[[nodiscard]] nlohmann::json ToJsonSchema(const TypeDescriptor& descriptor);
[[nodiscard]] nlohmann::json ToJson(const PropertyDiff& diff);
[[nodiscard]] nlohmann::json ToJson(const PropertyWriteResult& result);

} // namespace Astra::PropertySystem
