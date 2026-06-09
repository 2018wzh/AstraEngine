#include <Astra/Core/Error.hpp>

#include <utility>

namespace Astra::Core {

namespace {

Diagnostic MakeDiagnostic(std::string code, DiagnosticSeverity severity, std::string message) {
    Diagnostic diagnostic;
    diagnostic.code = std::move(code);
    diagnostic.category = "core.error";
    diagnostic.severity = severity;
    diagnostic.message = std::move(message);
    return diagnostic;
}

} // namespace

ErrorReporter::ErrorReporter(ErrorPolicy policy) : policy_(policy) {}

const ErrorPolicy& ErrorReporter::Policy() const {
    return policy_;
}

ErrorReport ErrorReporter::MakeRecoverable(ErrorCode code, std::string message, std::string diagnostic_code) const {
    auto diagnostic = MakeDiagnostic(std::move(diagnostic_code), DiagnosticSeverity::Error, message);
    return {ErrorSeverity::Recoverable, code, std::move(message), std::move(diagnostic)};
}

ErrorReport ErrorReporter::MakeFatal(ErrorCode code, std::string message, std::string diagnostic_code) const {
    auto diagnostic = MakeDiagnostic(std::move(diagnostic_code), DiagnosticSeverity::Fatal, message);
    return {ErrorSeverity::Fatal, code, std::move(message), std::move(diagnostic)};
}

ErrorReport ErrorReporter::MakeDeveloperAssert(std::string_view expression, std::string message) const {
    auto diagnostic = MakeDiagnostic("ASTRA_CORE_ASSERT", DiagnosticSeverity::Fatal, message);
    diagnostic.context["expression"] = std::string(expression);
    return {ErrorSeverity::DeveloperAssert, ErrorCode::InternalError, std::move(message), std::move(diagnostic)};
}

} // namespace Astra::Core
