#pragma once

#include <expected>
#include <string>

namespace astra {

struct Error {
    std::string code;
    std::string message;
};

template <typename T> using Expected = std::expected<T, Error>;

using VoidResult = std::expected<void, Error>;

inline Error make_error(std::string code, std::string message) {
    return Error{std::move(code), std::move(message)};
}

} // namespace astra
