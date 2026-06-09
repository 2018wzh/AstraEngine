#pragma once

#include <Astra/Core/Diagnostics.hpp>

#include <string>
#include <string_view>

namespace Astra::Core {

enum class ErrorSeverity {
    DeveloperAssert,
    Recoverable,
    Fatal
};

struct ErrorPolicy {
    bool break_on_developer_assert = true;
    bool throw_on_fatal = false;
};

struct ErrorReport {
    ErrorSeverity severity = ErrorSeverity::Recoverable;
    ErrorCode code = ErrorCode::InternalError;
    std::string message;
    Diagnostic diagnostic;
};

class ErrorReporter {
public:
    explicit ErrorReporter(ErrorPolicy policy = {});

    [[nodiscard]] const ErrorPolicy& Policy() const;
    [[nodiscard]] ErrorReport MakeRecoverable(ErrorCode code, std::string message, std::string diagnostic_code = "ASTRA_CORE_RECOVERABLE") const;
    [[nodiscard]] ErrorReport MakeFatal(ErrorCode code, std::string message, std::string diagnostic_code = "ASTRA_CORE_FATAL") const;
    [[nodiscard]] ErrorReport MakeDeveloperAssert(std::string_view expression, std::string message) const;

private:
    ErrorPolicy policy_;
};

} // namespace Astra::Core
