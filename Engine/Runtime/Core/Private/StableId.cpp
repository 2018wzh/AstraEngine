#include <Astra/Core/StableId.hpp>

#include <algorithm>
#include <cctype>

namespace Astra::Core {

namespace {

std::string NormalizeValue(std::string_view value) {
    std::string result(value);
    std::replace(result.begin(), result.end(), '\\', '/');
    while (result.find("//") != std::string::npos) {
        result.replace(result.find("//"), 2, "/");
    }
    if (!result.empty() && result.front() == '/') {
        result.erase(result.begin());
    }
    return result;
}

} // namespace

StableId::StableId(StableIdKind kind, std::string value) : kind_(kind), value_(NormalizeValue(value)) {}

std::string StableId::ToString() const {
    return ToPrefix(kind_) + ":/" + value_;
}

Result<StableId> ParseStableId(std::string_view text) {
    const auto split = text.find(":/");
    if (split == std::string_view::npos) {
        return Result<StableId>::Failure(ErrorCode::InvalidFormat, "stable id must contain ':/'.");
    }

    const auto prefix = text.substr(0, split);
    auto kind = StableIdKindFromPrefix(prefix);
    if (kind == StableIdKind::Unknown) {
        return Result<StableId>::Failure(ErrorCode::InvalidFormat, "stable id has unknown prefix.");
    }

    const auto value = NormalizeValue(text.substr(split + 2));
    if (value.empty() || value.find("..") != std::string::npos) {
        return Result<StableId>::Failure(ErrorCode::InvalidFormat, "stable id path is empty or escapes its root.");
    }

    return Result<StableId>::Success(StableId(kind, value));
}

std::string ToPrefix(StableIdKind kind) {
    switch (kind) {
    case StableIdKind::Type:
        return "type";
    case StableIdKind::Property:
        return "property";
    case StableIdKind::Asset:
        return "asset";
    case StableIdKind::Actor:
        return "actor";
    case StableIdKind::Component:
        return "component";
    case StableIdKind::EventType:
        return "event";
    case StableIdKind::Task:
        return "task";
    case StableIdKind::StateMachine:
        return "state_machine";
    case StableIdKind::Provider:
        return "provider";
    case StableIdKind::Unknown:
        break;
    }
    return "unknown";
}

StableIdKind StableIdKindFromPrefix(std::string_view prefix) {
    if (prefix == "type") {
        return StableIdKind::Type;
    }
    if (prefix == "property") {
        return StableIdKind::Property;
    }
    if (prefix == "asset" || prefix == "native" || prefix.starts_with("foreign-") || prefix == "virtual" || prefix == "package") {
        return StableIdKind::Asset;
    }
    if (prefix == "actor") {
        return StableIdKind::Actor;
    }
    if (prefix == "component") {
        return StableIdKind::Component;
    }
    if (prefix == "event") {
        return StableIdKind::EventType;
    }
    if (prefix == "task") {
        return StableIdKind::Task;
    }
    if (prefix == "state_machine") {
        return StableIdKind::StateMachine;
    }
    if (prefix == "provider") {
        return StableIdKind::Provider;
    }
    return StableIdKind::Unknown;
}

} // namespace Astra::Core
