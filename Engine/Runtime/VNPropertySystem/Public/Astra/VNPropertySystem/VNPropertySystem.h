#pragma once

#include <Astra/Core/Diagnostics.h>
#include <Astra/Core/Result.h>

#include <nlohmann/json.hpp>

#include <string>
#include <unordered_map>
#include <vector>

namespace astra {

enum class VNPropertyKind {
    Boolean,
    Integer,
    Number,
    String,
    LocalizedText,
    AssetRef,
    Enum,
};

struct VNPropertyDescriptor {
    std::string id;
    VNPropertyKind kind = VNPropertyKind::String;
    nlohmann::json default_value;
    bool ai_editable = false;
    bool tool_generated = false;
    bool read_only = false;
    bool requires_review = false;
    std::vector<std::string> enum_values;
};

struct VNTypeDescriptor {
    std::string type_id;
    std::string display_name;
    std::vector<VNPropertyDescriptor> properties;
};

class VNPropertyRegistry {
  public:
    VoidResult register_type(VNTypeDescriptor descriptor, DiagnosticSink& diagnostics);
    [[nodiscard]] nlohmann::json generate_json_schema(std::string_view type_id) const;
    [[nodiscard]] bool contains(std::string_view type_id) const;

  private:
    std::unordered_map<std::string, VNTypeDescriptor> types_;
};

} // namespace astra
