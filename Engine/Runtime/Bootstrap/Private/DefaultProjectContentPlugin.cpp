#include <Astra/Bootstrap/RuntimeProviderRegistry.h>

#include <Astra/Core/Path.h>

#include <yaml-cpp/yaml.h>

namespace astra {

namespace {

std::filesystem::path find_project_manifest(const std::filesystem::path& project_root) {
    if (std::filesystem::is_regular_file(project_root)) {
        return project_root;
    }
    for (const auto& entry : std::filesystem::directory_iterator(project_root)) {
        if (entry.is_regular_file() && entry.path().filename().string().ends_with(".vnproj.yaml")) {
            return entry.path();
        }
    }
    return {};
}

class DefaultProjectContentProvider final : public IProjectContentProvider {
public:
    [[nodiscard]] std::string id() const override {
        return "astra.content.default_project";
    }

    [[nodiscard]] Expected<ProjectContentLoadResult>
    load_project_content(const std::filesystem::path& project_root,
                         Vfs& vfs,
                         AssetRegistry& asset_registry,
                         DiagnosticSink& diagnostics) const override {
        const std::filesystem::path resolved_project_root =
            std::filesystem::is_regular_file(project_root) ? project_root.parent_path()
                                                           : project_root;
        const auto manifest_path = find_project_manifest(project_root);
        if (manifest_path.empty()) {
            return std::unexpected(
                make_error("project.manifest_missing", "No .vnproj.yaml found under project root"));
        }

        const YAML::Node manifest = YAML::LoadFile(path_to_utf8(manifest_path));
        std::string entry_script = "Content/Scripts/main.astra";
        if (manifest["runtime"] && manifest["runtime"]["entry_script"]) {
            entry_script = manifest["runtime"]["entry_script"].as<std::string>();
        }

        ProjectContentLoadResult result;
        result.project_root = resolved_project_root;
        result.content_root = resolved_project_root / "Content";
        result.entry_script = resolved_project_root / entry_script;

        if (auto mounted = vfs.mount_directory("Content", result.content_root, true); !mounted) {
            diagnostics.error(mounted.error().code, mounted.error().message);
            return std::unexpected(mounted.error());
        }
        if (auto loaded_assets = asset_registry.load_sidecars(result.content_root, diagnostics);
            !loaded_assets) {
            return std::unexpected(loaded_assets.error());
        }
        return result;
    }
};

} // namespace

VoidResult register_default_project_content_plugin(RuntimeProviderRegistry& registry,
                                                   DiagnosticSink& diagnostics) {
    return registry.register_project_content_provider(
        std::make_shared<DefaultProjectContentProvider>(), diagnostics);
}

} // namespace astra
