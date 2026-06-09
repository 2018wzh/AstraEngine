#include <Astra/Core/Path.hpp>

#include <algorithm>

namespace Astra::Core {

std::string VirtualPath::ToString() const {
    std::string prefix;
    switch (root) {
    case VirtualRoot::Project:
        prefix = "project";
        break;
    case VirtualRoot::Package:
        prefix = "package";
        break;
    case VirtualRoot::User:
        prefix = "user";
        break;
    case VirtualRoot::Cache:
        prefix = "cache";
        break;
    case VirtualRoot::Foreign:
        prefix = mount;
        break;
    }
    return prefix + ":/" + path;
}

Result<VirtualPath> ParseVirtualPath(std::string_view text) {
    const auto split = text.find(":/");
    if (split == std::string_view::npos) {
        return Result<VirtualPath>::Failure(ErrorCode::InvalidFormat, "virtual path must contain ':/'.");
    }

    const auto prefix = text.substr(0, split);
    std::string path(text.substr(split + 2));
    std::replace(path.begin(), path.end(), '\\', '/');
    if (path.empty() || path.find("..") != std::string::npos) {
        return Result<VirtualPath>::Failure(ErrorCode::InvalidFormat, "virtual path is empty or escapes its root.");
    }

    VirtualPath result;
    if (prefix == "project") {
        result.root = VirtualRoot::Project;
    } else if (prefix == "package") {
        result.root = VirtualRoot::Package;
    } else if (prefix == "user") {
        result.root = VirtualRoot::User;
    } else if (prefix == "cache") {
        result.root = VirtualRoot::Cache;
    } else if (prefix.starts_with("foreign-")) {
        result.root = VirtualRoot::Foreign;
        result.mount = std::string(prefix);
    } else {
        return Result<VirtualPath>::Failure(ErrorCode::InvalidFormat, "virtual path has unknown root.");
    }
    result.path = std::move(path);
    return Result<VirtualPath>::Success(result);
}

std::filesystem::path NormalizeNativePath(const std::filesystem::path& path) {
    return path.lexically_normal();
}

} // namespace Astra::Core

