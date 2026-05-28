#pragma once

#include <cstdint>
#include <string>
#include <vector>

namespace astra {

enum class DiagnosticSeverity : std::uint32_t {
    Info = 0,
    Warning = 1,
    Error = 2,
};

struct Diagnostic {
    DiagnosticSeverity severity = DiagnosticSeverity::Info;
    std::string code;
    std::string message;
};

class DiagnosticSink {
  public:
    void info(std::string code, std::string message);
    void warning(std::string code, std::string message);
    void error(std::string code, std::string message);
    void emit(Diagnostic diagnostic);

    [[nodiscard]] bool has_errors() const;
    [[nodiscard]] const std::vector<Diagnostic>& diagnostics() const;
    void clear();

  private:
    std::vector<Diagnostic> diagnostics_;
};

} // namespace astra
