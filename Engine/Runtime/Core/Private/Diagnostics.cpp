#include <Astra/Core/Diagnostics.h>

namespace astra {

void DiagnosticSink::info(std::string code, std::string message) {
    emit({DiagnosticSeverity::Info, std::move(code), std::move(message)});
}

void DiagnosticSink::warning(std::string code, std::string message) {
    emit({DiagnosticSeverity::Warning, std::move(code), std::move(message)});
}

void DiagnosticSink::error(std::string code, std::string message) {
    emit({DiagnosticSeverity::Error, std::move(code), std::move(message)});
}

void DiagnosticSink::emit(Diagnostic diagnostic) {
    diagnostics_.push_back(std::move(diagnostic));
}

bool DiagnosticSink::has_errors() const {
    for (const Diagnostic& diagnostic : diagnostics_) {
        if (diagnostic.severity == DiagnosticSeverity::Error) {
            return true;
        }
    }
    return false;
}

const std::vector<Diagnostic>& DiagnosticSink::diagnostics() const {
    return diagnostics_;
}

void DiagnosticSink::clear() {
    diagnostics_.clear();
}

} // namespace astra
