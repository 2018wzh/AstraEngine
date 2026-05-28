#pragma once

#include <filesystem>
#include <string>

namespace astra {

std::filesystem::path normalize_path(const std::filesystem::path& path);
std::string path_to_utf8(const std::filesystem::path& path);
std::filesystem::path utf8_to_path(std::string_view path);

} // namespace astra
