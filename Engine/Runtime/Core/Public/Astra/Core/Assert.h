#pragma once

#include <stdexcept>
#include <string>
#include <string_view>

namespace astra {

class FatalError final : public std::runtime_error {
  public:
    FatalError(std::string code, std::string message);

    [[nodiscard]] const std::string& code() const;
    [[nodiscard]] const std::string& message() const;

  private:
    std::string code_;
    std::string message_;
};

using FatalErrorHandler = void (*)(std::string_view code, std::string_view message);

void set_fatal_error_handler(FatalErrorHandler handler);
void fatal_error(std::string code, std::string message);
void assert_condition(bool condition, std::string code, std::string message);

} // namespace astra
