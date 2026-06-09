#pragma once

#include <Astra/Core/Types.hpp>
#include <nlohmann/json.hpp>

#include <string>
#include <unordered_map>
#include <vector>

namespace Astra::Core {

enum class DiagnosticSeverity {
    Info,
    Warning,
    Error,
    Blocking,
    Fatal
};

struct SourceLocation {
    std::string file;
    u32 line = 0;
    u32 column = 0;
};

struct DiagnosticObject {
    std::string kind;
    std::string id;
};

struct Diagnostic {
    std::string code;
    std::string category;
    DiagnosticSeverity severity = DiagnosticSeverity::Info;
    std::string message;
    SourceLocation source;
    std::vector<DiagnosticObject> objects;
    std::unordered_map<std::string, std::string> context;
    std::vector<std::string> suggested_fixes;

    [[nodiscard]] bool BlocksRelease() const;
};

struct DiagnosticCodeDescriptor {
    std::string code;
    std::string category;
    DiagnosticSeverity minimum_release_severity = DiagnosticSeverity::Error;
    bool registered_for_release = true;
};

class DiagnosticCodeRegistry {
public:
    [[nodiscard]] Result<void> Register(DiagnosticCodeDescriptor descriptor);
    [[nodiscard]] const DiagnosticCodeDescriptor* Find(std::string_view code) const;
    [[nodiscard]] bool Contains(std::string_view code) const;
    [[nodiscard]] std::vector<std::string> Codes() const;

private:
    std::vector<DiagnosticCodeDescriptor> descriptors_;
};

enum class ReleaseProfile {
    Development,
    Deterministic,
    Shipping
};

struct ReleasePolicy {
    ReleaseProfile profile = ReleaseProfile::Development;
    bool block_on_error = false;
    bool require_registered_codes = true;
};

struct FoundationGateReport {
    std::string schema = "astra.foundation.gate.v1";
    bool passed = true;
    ReleasePolicy policy;
    std::vector<Diagnostic> diagnostics;
    std::vector<std::string> registered_codes;
};

class DiagnosticSink {
public:
    void Emit(Diagnostic diagnostic);
    [[nodiscard]] bool HasBlocking() const;
    [[nodiscard]] const std::vector<Diagnostic>& Diagnostics() const;
    void Clear();

private:
    std::vector<Diagnostic> diagnostics_;
};

[[nodiscard]] std::string ToString(DiagnosticSeverity severity);
[[nodiscard]] std::string ToString(ReleaseProfile profile);
[[nodiscard]] DiagnosticSeverity DiagnosticSeverityFromString(std::string_view value);
[[nodiscard]] nlohmann::json ToJson(const Diagnostic& diagnostic);
[[nodiscard]] nlohmann::json ToJson(const DiagnosticCodeDescriptor& descriptor);
[[nodiscard]] nlohmann::json ToJson(const ReleasePolicy& policy);
[[nodiscard]] nlohmann::json ToJson(const FoundationGateReport& report);
[[nodiscard]] FoundationGateReport EvaluateFoundationGate(const DiagnosticSink& diagnostics, const DiagnosticCodeRegistry& registry, ReleasePolicy policy);

} // namespace Astra::Core
