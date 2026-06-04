#pragma once

#include <Astra/Core/Diagnostics.h>
#include <Astra/Core/Result.h>

#include <nlohmann/json.hpp>

#include <cstdint>
#include <string>
#include <unordered_map>
#include <vector>

namespace astra {

using TypeId = std::string;
using PropertyId = std::string;

enum class PropertyTypeKind : std::uint32_t {
    Boolean,
    Integer,
    Number,
    String,
    Enum,
    Array,
    Struct,
};

enum class PropertyFlags : std::uint32_t {
    None = 0,
    AiEditable = 1u << 0,
    ToolGenerated = 1u << 1,
    ReadOnly = 1u << 2,
    RequiresReview = 1u << 3,
};

inline constexpr PropertyFlags operator|(PropertyFlags lhs, PropertyFlags rhs) {
    return static_cast<PropertyFlags>(static_cast<std::uint32_t>(lhs) |
                                      static_cast<std::uint32_t>(rhs));
}

inline constexpr bool has_flag(PropertyFlags value, PropertyFlags flag) {
    return (static_cast<std::uint32_t>(value) & static_cast<std::uint32_t>(flag)) != 0;
}

struct PropertyDescriptor {
    PropertyId id;
    PropertyTypeKind kind = PropertyTypeKind::String;
    nlohmann::json default_value;
    PropertyFlags flags = PropertyFlags::None;
    std::vector<std::string> enum_values;
    TypeId value_type;
};

struct TypeDescriptor {
    TypeId id;
    std::string display_name;
    std::vector<PropertyDescriptor> properties;
};

class PropertyRegistry {
  public:
    VoidResult register_type(TypeDescriptor descriptor, DiagnosticSink& diagnostics);
    [[nodiscard]] bool contains(std::string_view type_id) const;
    [[nodiscard]] const TypeDescriptor* find(std::string_view type_id) const;
    [[nodiscard]] nlohmann::json generate_json_schema(std::string_view type_id) const;

  private:
    std::unordered_map<std::string, TypeDescriptor> types_;
};

} // namespace astra
