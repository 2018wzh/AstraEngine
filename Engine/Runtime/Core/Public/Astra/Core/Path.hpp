#pragma once

#include <Astra/Core/Export.hpp>
#include <Astra/Core/Types.hpp>

#include <filesystem>
#include <string>
#include <string_view>

namespace Astra::Core {

enum class VirtualRoot {
    Project,
    Package,
    User,
    Cache,
    Foreign
};

struct VirtualPath {
    VirtualRoot root = VirtualRoot::Project;
    std::string mount;
    std::string path;

    [[nodiscard]] ASTRA_CORE_API std::string ToString() const;
};

[[nodiscard]] ASTRA_CORE_API Result<VirtualPath> ParseVirtualPath(std::string_view text);
[[nodiscard]] ASTRA_CORE_API std::filesystem::path NormalizeNativePath(const std::filesystem::path& path);

} // namespace Astra::Core
