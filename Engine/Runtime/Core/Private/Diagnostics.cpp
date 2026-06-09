#include <Astra/Core/Diagnostics.hpp>

#include <algorithm>
#include <ranges>

namespace Astra::Core {

namespace {

int SeverityRank(DiagnosticSeverity severity) {
    switch (severity) {
    case DiagnosticSeverity::Info:
        return 0;
    case DiagnosticSeverity::Warning:
        return 1;
    case DiagnosticSeverity::Error:
        return 2;
    case DiagnosticSeverity::Blocking:
        return 3;
    case DiagnosticSeverity::Fatal:
        return 4;
    }
    return 2;
}

} // namespace

bool Diagnostic::BlocksRelease() const {
    return severity == DiagnosticSeverity::Blocking || severity == DiagnosticSeverity::Fatal;
}

Astra::Core::Result<void> DiagnosticCodeRegistry::Register(DiagnosticCodeDescriptor descriptor) {
    if (descriptor.code.empty() || descriptor.category.empty()) {
        return Result<void>::Failure(ErrorCode::InvalidArgument, "diagnostic code and category are required");
    }
    if (Contains(descriptor.code)) {
        return Result<void>::Failure(ErrorCode::InvalidArgument, "diagnostic code already registered");
    }
    descriptors_.push_back(std::move(descriptor));
    return Result<void>::Success();
}

const DiagnosticCodeDescriptor* DiagnosticCodeRegistry::Find(std::string_view code) const {
    auto it = std::ranges::find_if(descriptors_, [&](const DiagnosticCodeDescriptor& descriptor) { return descriptor.code == code; });
    return it == descriptors_.end() ? nullptr : &*it;
}

bool DiagnosticCodeRegistry::Contains(std::string_view code) const {
    return Find(code) != nullptr;
}

std::vector<std::string> DiagnosticCodeRegistry::Codes() const {
    std::vector<std::string> codes;
    codes.reserve(descriptors_.size());
    for (const auto& descriptor : descriptors_) {
        codes.push_back(descriptor.code);
    }
    return codes;
}

void DiagnosticSink::Emit(Diagnostic diagnostic) {
    diagnostics_.push_back(std::move(diagnostic));
}

bool DiagnosticSink::HasBlocking() const {
    return std::ranges::any_of(diagnostics_, [](const Diagnostic& diagnostic) { return diagnostic.BlocksRelease(); });
}

const std::vector<Diagnostic>& DiagnosticSink::Diagnostics() const {
    return diagnostics_;
}

void DiagnosticSink::Clear() {
    diagnostics_.clear();
}

std::string ToString(DiagnosticSeverity severity) {
    switch (severity) {
    case DiagnosticSeverity::Info:
        return "info";
    case DiagnosticSeverity::Warning:
        return "warning";
    case DiagnosticSeverity::Error:
        return "error";
    case DiagnosticSeverity::Blocking:
        return "blocking";
    case DiagnosticSeverity::Fatal:
        return "fatal";
    }
    return "error";
}

std::string ToString(ReleaseProfile profile) {
    switch (profile) {
    case ReleaseProfile::Development:
        return "development";
    case ReleaseProfile::Deterministic:
        return "deterministic";
    case ReleaseProfile::Shipping:
        return "shipping";
    }
    return "development";
}

DiagnosticSeverity DiagnosticSeverityFromString(std::string_view value) {
    if (value == "info") {
        return DiagnosticSeverity::Info;
    }
    if (value == "warning") {
        return DiagnosticSeverity::Warning;
    }
    if (value == "blocking") {
        return DiagnosticSeverity::Blocking;
    }
    if (value == "fatal") {
        return DiagnosticSeverity::Fatal;
    }
    return DiagnosticSeverity::Error;
}

nlohmann::json ToJson(const DiagnosticCodeDescriptor& descriptor) {
    return {
        {"code", descriptor.code},
        {"category", descriptor.category},
        {"minimum_release_severity", ToString(descriptor.minimum_release_severity)},
        {"registered_for_release", descriptor.registered_for_release},
    };
}

nlohmann::json ToJson(const ReleasePolicy& policy) {
    return {
        {"profile", ToString(policy.profile)},
        {"block_on_error", policy.block_on_error},
        {"require_registered_codes", policy.require_registered_codes},
    };
}

nlohmann::json ToJson(const FoundationGateReport& report) {
    nlohmann::json diagnostics = nlohmann::json::array();
    for (const auto& diagnostic : report.diagnostics) {
        diagnostics.push_back(ToJson(diagnostic));
    }
    return {
        {"schema", report.schema},
        {"passed", report.passed},
        {"policy", ToJson(report.policy)},
        {"diagnostics", diagnostics},
        {"registered_codes", report.registered_codes},
    };
}

nlohmann::json ToJson(const Diagnostic& diagnostic) {
    nlohmann::json objects = nlohmann::json::array();
    for (const auto& object : diagnostic.objects) {
        objects.push_back({{"kind", object.kind}, {"id", object.id}});
    }

    return {
        {"code", diagnostic.code},
        {"category", diagnostic.category},
        {"severity", ToString(diagnostic.severity)},
        {"message", diagnostic.message},
        {"source", {{"file", diagnostic.source.file}, {"line", diagnostic.source.line}, {"column", diagnostic.source.column}}},
        {"objects", objects},
        {"context", diagnostic.context},
        {"suggested_fixes", diagnostic.suggested_fixes},
    };
}

FoundationGateReport EvaluateFoundationGate(const DiagnosticSink& diagnostics, const DiagnosticCodeRegistry& registry, ReleasePolicy policy) {
    FoundationGateReport report;
    report.policy = policy;
    report.registered_codes = registry.Codes();
    bool failed = false;
    for (const auto& diagnostic : diagnostics.Diagnostics()) {
        bool include = false;
        if (diagnostic.BlocksRelease()) {
            include = true;
            failed = true;
        }
        if (policy.block_on_error && diagnostic.severity == DiagnosticSeverity::Error) {
            include = true;
            failed = true;
        }
        const auto* descriptor = registry.Find(diagnostic.code);
        if (descriptor != nullptr && descriptor->registered_for_release && SeverityRank(diagnostic.severity) >= SeverityRank(descriptor->minimum_release_severity)) {
            include = true;
            failed = true;
        }
        if (policy.require_registered_codes && descriptor == nullptr) {
            Diagnostic missing;
            missing.code = "ASTRA_DIAGNOSTIC_CODE_UNREGISTERED";
            missing.category = "core.diagnostics";
            missing.severity = DiagnosticSeverity::Blocking;
            missing.message = "Diagnostic code is not registered for release gate evaluation.";
            missing.objects = {{"DiagnosticCode", diagnostic.code}};
            report.diagnostics.push_back(std::move(missing));
            include = true;
            failed = true;
        }
        if (include) {
            report.diagnostics.push_back(diagnostic);
        }
    }
    report.passed = !failed;
    return report;
}

} // namespace Astra::Core
