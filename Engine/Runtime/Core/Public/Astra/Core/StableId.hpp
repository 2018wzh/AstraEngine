#pragma once

#include <Astra/Core/Types.hpp>

#include <compare>
#include <string>
#include <string_view>

namespace Astra::Core {

enum class StableIdKind {
    Type,
    Property,
    Asset,
    Actor,
    Component,
    EventType,
    Task,
    StateMachine,
    Provider,
    Unknown
};

class StableId {
public:
    StableId() = default;
    StableId(StableIdKind kind, std::string value);

    [[nodiscard]] StableIdKind Kind() const { return kind_; }
    [[nodiscard]] const std::string& Value() const { return value_; }
    [[nodiscard]] std::string ToString() const;
    [[nodiscard]] bool Empty() const { return value_.empty(); }

    friend bool operator==(const StableId&, const StableId&) = default;
    friend auto operator<=>(const StableId&, const StableId&) = default;

private:
    StableIdKind kind_ = StableIdKind::Unknown;
    std::string value_;
};

using TypeId = StableId;
using PropertyId = StableId;
using AssetId = StableId;
using ActorId = StableId;
using ComponentId = StableId;
using EventTypeId = StableId;
using ProviderId = StableId;

[[nodiscard]] Result<StableId> ParseStableId(std::string_view text);
[[nodiscard]] std::string ToPrefix(StableIdKind kind);
[[nodiscard]] StableIdKind StableIdKindFromPrefix(std::string_view prefix);

} // namespace Astra::Core

template <>
struct std::hash<Astra::Core::StableId> {
    std::size_t operator()(const Astra::Core::StableId& id) const noexcept {
        return std::hash<std::string>()(id.ToString());
    }
};

