#include <Astra/Core/Serialization.hpp>

#include <algorithm>
#include <set>

namespace Astra::Core {

void MigrationRegistry::Register(MigrationRule rule) {
    rules_[{rule.schema, rule.from_version, rule.to_version}] = std::move(rule);
}

Result<VersionedDocument> MigrationRegistry::Migrate(const VersionedDocument& document, u32 target_version, DiagnosticSink& diagnostics) const {
    VersionedDocument current = document;
    while (current.version < target_version) {
        const auto key = std::tuple{current.schema, current.version, current.version + 1};
        auto it = rules_.find(key);
        if (it == rules_.end()) {
            Diagnostic diagnostic;
            diagnostic.code = "ASTRA_CORE_MIGRATION_MISSING";
            diagnostic.category = "core.serialization";
            diagnostic.severity = DiagnosticSeverity::Blocking;
            diagnostic.message = "Missing migration rule.";
            diagnostic.objects = {{"schema", current.schema}};
            diagnostics.Emit(std::move(diagnostic));
            return Result<VersionedDocument>::Failure(ErrorCode::NotFound, "missing migration rule");
        }
        current.payload = it->second.migrate ? it->second.migrate(current.payload) : current.payload;
        auto policy_result = ApplyUnknownFieldPolicy(current.payload, it->second, diagnostics);
        if (policy_result.blocking) {
            return Result<VersionedDocument>::Failure(ErrorCode::InvalidFormat, "unknown fields violate migration policy");
        }
        current.version = it->second.to_version;
    }
    return Result<VersionedDocument>::Success(current);
}

namespace {

std::string ToString(UnknownFieldPolicy policy) {
    switch (policy) {
    case UnknownFieldPolicy::Preserve:
        return "preserve";
    case UnknownFieldPolicy::Warn:
        return "warn";
    case UnknownFieldPolicy::Error:
        return "error";
    case UnknownFieldPolicy::Drop:
        return "drop";
    }
    return "preserve";
}

} // namespace

nlohmann::json ToJson(const VersionedDocument& document) {
    return {
        {"schema", document.schema},
        {"version", document.version},
        {"object_id", document.object_id},
        {"payload", document.payload},
    };
}

nlohmann::json ToJson(const UnknownFieldPolicyResult& result) {
    return {
        {"policy", ToString(result.policy)},
        {"unknown_fields", result.unknown_fields},
        {"blocking", result.blocking},
    };
}

Result<VersionedDocument> VersionedDocumentFromJson(const nlohmann::json& json) {
    if (!json.contains("schema") || !json.contains("version") || !json.contains("payload")) {
        return Result<VersionedDocument>::Failure(ErrorCode::InvalidFormat, "versioned document requires schema, version, and payload.");
    }
    VersionedDocument document;
    document.schema = json.at("schema").get<std::string>();
    document.version = json.at("version").get<u32>();
    document.object_id = json.value("object_id", "");
    document.payload = json.at("payload");
    return Result<VersionedDocument>::Success(document);
}

UnknownFieldPolicyResult ApplyUnknownFieldPolicy(nlohmann::json& payload, const MigrationRule& rule, DiagnosticSink& diagnostics) {
    UnknownFieldPolicyResult result;
    result.policy = rule.unknown_field_policy;
    if (rule.known_fields_after_migration.empty() || !payload.is_object()) {
        return result;
    }

    const std::set<std::string> known(rule.known_fields_after_migration.begin(), rule.known_fields_after_migration.end());
    for (auto it = payload.begin(); it != payload.end();) {
        if (known.contains(it.key())) {
            ++it;
            continue;
        }

        result.unknown_fields.push_back(it.key());
        if (rule.unknown_field_policy == UnknownFieldPolicy::Drop) {
            it = payload.erase(it);
            continue;
        }
        ++it;
    }

    if (result.unknown_fields.empty()) {
        return result;
    }

    if (rule.unknown_field_policy == UnknownFieldPolicy::Warn || rule.unknown_field_policy == UnknownFieldPolicy::Error) {
        Diagnostic diagnostic;
        diagnostic.code = rule.diagnostic_code.empty() ? "ASTRA_CORE_UNKNOWN_FIELD" : rule.diagnostic_code;
        diagnostic.category = "core.serialization";
        diagnostic.severity = rule.unknown_field_policy == UnknownFieldPolicy::Error ? DiagnosticSeverity::Blocking : DiagnosticSeverity::Warning;
        diagnostic.message = "Versioned document contains fields not declared by the migration target schema.";
        diagnostic.objects = {{"schema", rule.schema}};
        diagnostic.context["policy"] = ToString(rule.unknown_field_policy);
        diagnostic.context["fields"] = nlohmann::json(result.unknown_fields).dump();
        diagnostics.Emit(std::move(diagnostic));
        result.blocking = rule.unknown_field_policy == UnknownFieldPolicy::Error;
    }

    return result;
}

} // namespace Astra::Core
