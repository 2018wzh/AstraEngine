#include <Astra/Core/Path.h>

#include <algorithm>

namespace astra {

std::filesystem::path normalize_path(const std::filesystem::path& path) {
    return path.lexically_normal();
}

std::string path_to_utf8(const std::filesystem::path& path) {
    const std::u8string value = path.generic_u8string();
    return std::string(value.begin(), value.end());
}

std::filesystem::path utf8_to_path(std::string_view path) {
    return std::filesystem::path(std::string(path));
}

} // namespace astra
