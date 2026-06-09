#pragma once

#include <cstdint>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <utility>

namespace Astra::Core {

using u8 = std::uint8_t;
using u16 = std::uint16_t;
using u32 = std::uint32_t;
using u64 = std::uint64_t;
using i8 = std::int8_t;
using i16 = std::int16_t;
using i32 = std::int32_t;
using i64 = std::int64_t;

enum class ErrorCode {
    Ok,
    InvalidArgument,
    InvalidFormat,
    NotFound,
    PermissionDenied,
    VersionMismatch,
    DependencyCycle,
    Unsupported,
    InternalError
};

template <typename T>
class Result {
public:
    static Result Success(T value) { return Result(std::move(value)); }
    static Result Failure(ErrorCode code, std::string message) { return Result(code, std::move(message)); }

    [[nodiscard]] bool HasValue() const { return value_.has_value(); }
    [[nodiscard]] explicit operator bool() const { return HasValue(); }
    [[nodiscard]] const T& Value() const { return *value_; }
    [[nodiscard]] T& Value() { return *value_; }
    [[nodiscard]] ErrorCode Error() const { return error_; }
    [[nodiscard]] const std::string& Message() const { return message_; }

private:
    explicit Result(T value) : value_(std::move(value)), error_(ErrorCode::Ok) {}
    Result(ErrorCode code, std::string message) : error_(code), message_(std::move(message)) {}

    std::optional<T> value_;
    ErrorCode error_ = ErrorCode::InternalError;
    std::string message_;
};

template <>
class Result<void> {
public:
    static Result Success() { return Result(); }
    static Result Failure(ErrorCode code, std::string message) { return Result(code, std::move(message)); }

    [[nodiscard]] bool HasValue() const { return error_ == ErrorCode::Ok; }
    [[nodiscard]] explicit operator bool() const { return HasValue(); }
    [[nodiscard]] ErrorCode Error() const { return error_; }
    [[nodiscard]] const std::string& Message() const { return message_; }

private:
    Result() = default;
    Result(ErrorCode code, std::string message) : error_(code), message_(std::move(message)) {}

    ErrorCode error_ = ErrorCode::Ok;
    std::string message_;
};

} // namespace Astra::Core

