#include <Astra/VFS/DirectoryMount.h>

#include <Astra/Core/Path.h>

#include <filesystem>

namespace astra {

VoidResult Vfs::mount_directory(std::string name, std::filesystem::path root, bool read_only) {
    root = normalize_path(root);
    if (!std::filesystem::exists(root) || !std::filesystem::is_directory(root)) {
        return std::unexpected(make_error(
            "vfs.mount_missing", "Directory mount root does not exist: " + path_to_utf8(root)));
    }
    mounts_.push_back({std::move(name), std::move(root), read_only});
    return {};
}

std::optional<std::filesystem::path> Vfs::resolve(std::string_view logical_path) const {
    std::filesystem::path logical{std::string(logical_path)};
    for (auto it = mounts_.rbegin(); it != mounts_.rend(); ++it) {
        auto candidate = normalize_path(it->root / logical);
        if (std::filesystem::exists(candidate)) {
            return candidate;
        }
    }
    return std::nullopt;
}

const std::vector<MountedDirectory>& Vfs::mounts() const {
    return mounts_;
}

} // namespace astra
