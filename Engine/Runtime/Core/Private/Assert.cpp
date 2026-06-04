#include <Astra/Core/Assert.h>

#include <utility>

namespace astra {

namespace {

FatalErrorHandler g_fatal_error_handler = nullptr;

} // namespace

FatalError::FatalError(std::string code, std::string message)
    : std::runtime_error(code + ": " + message), code_(std::move(code)),
      message_(std::move(message)) {}

const std::string& FatalError::code() const {
    return code_;
}

const std::string& FatalError::message() const {
    return message_;
}

void set_fatal_error_handler(FatalErrorHandler handler) {
    g_fatal_error_handler = handler;
}

void fatal_error(std::string code, std::string message) {
    if (g_fatal_error_handler != nullptr) {
        g_fatal_error_handler(code, message);
    }
    throw FatalError(std::move(code), std::move(message));
}

void assert_condition(bool condition, std::string code, std::string message) {
    if (!condition) {
        fatal_error(std::move(code), std::move(message));
    }
}

} // namespace astra
