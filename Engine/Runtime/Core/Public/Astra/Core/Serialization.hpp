#pragma once

#include <Astra/Core/Export.hpp>
#include <Astra/Core/Diagnostics.hpp>
#include <nlohmann/json.hpp>

#include <functional>
#include <map>
#include <string>
#include <vector>

namespace Astra::Core {

enum class UnknownFieldPolicy {
    Preserve,
    Warn,
    Error,
    Drop
};

struct VersionedDocument {
    std::string schema;
    u32 version = 1;
    std::string object_id;
    nlohmann::json payload = nlohmann::json::object();
};

using MigrationFunction = std::function<nlohmann::json(const nlohmann::json&)>;

struct MigrationRule {
    std::string schema;
    u32 from_version = 0;
    u32 to_version = 0;
    UnknownFieldPolicy unknown_field_policy = UnknownFieldPolicy::Preserve;
    std::string diagnostic_code;
    std::vector<std::string> known_fields_after_migration;
    MigrationFunction migrate;
};

struct UnknownFieldPolicyResult {
    UnknownFieldPolicy policy = UnknownFieldPolicy::Preserve;
    std::vector<std::string> unknown_fields;
    bool blocking = false;
};

class ASTRA_CORE_API MigrationRegistry {
public:
    void Register(MigrationRule rule);
    [[nodiscard]] Result<VersionedDocument> Migrate(const VersionedDocument& document, u32 target_version, DiagnosticSink& diagnostics) const;

private:
    std::map<std::tuple<std::string, u32, u32>, MigrationRule> rules_;
};

[[nodiscard]] ASTRA_CORE_API nlohmann::json ToJson(const VersionedDocument& document);
[[nodiscard]] ASTRA_CORE_API nlohmann::json ToJson(const UnknownFieldPolicyResult& result);
[[nodiscard]] ASTRA_CORE_API Result<VersionedDocument> VersionedDocumentFromJson(const nlohmann::json& json);
[[nodiscard]] ASTRA_CORE_API UnknownFieldPolicyResult ApplyUnknownFieldPolicy(nlohmann::json& payload, const MigrationRule& rule, DiagnosticSink& diagnostics);

} // namespace Astra::Core
