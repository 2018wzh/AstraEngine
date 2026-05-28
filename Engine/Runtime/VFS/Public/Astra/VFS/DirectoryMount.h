#pragma once

#include <Astra/Core/Result.h>

#include <filesystem>
#include <optional>
#include <string>
#include <vector>

namespace astra {

struct MountedDirectory {
    std::string name;
    std::filesystem::path root;
    bool read_only = true;
};

class Vfs {
  public:
    VoidResult mount_directory(std::string name, std::filesystem::path root, bool read_only);
    [[nodiscard]] std::optional<std::filesystem::path> resolve(std::string_view logical_path) const;
    [[nodiscard]] const std::vector<MountedDirectory>& mounts() const;

  private:
    std::vector<MountedDirectory> mounts_;
};

} // namespace astra
