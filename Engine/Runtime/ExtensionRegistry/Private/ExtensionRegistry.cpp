#include <Astra/ExtensionRegistry/ExtensionRegistry.h>

namespace astra {

VoidResult ExtensionRegistry::register_extension(
    ExtensionDescriptor descriptor, const std::unordered_set<std::string>& capabilities,
    const std::unordered_set<std::string>& permissions, DiagnosticSink& diagnostics) {
    if (descriptor.id.empty()) {
        diagnostics.error("extension.empty_id", "Extension id must not be empty");
        return std::unexpected(make_error("extension.empty_id", "Extension id must not be empty"));
    }
    if (extensions_.contains(descriptor.id)) {
        diagnostics.error("extension.duplicate", "Duplicate extension id: " + descriptor.id);
        return std::unexpected(make_error("extension.duplicate", "Duplicate extension id"));
    }

    if (descriptor.required_capability.empty()) {
        descriptor.required_capability = default_capability_for_extension(descriptor.kind);
    }
    if (!descriptor.required_capability.empty() &&
        !capabilities.contains(descriptor.required_capability)) {
        diagnostics.error("extension.missing_capability",
                          descriptor.module_id + " tried to register " + descriptor.id +
                              " without capability " + descriptor.required_capability);
        return std::unexpected(
            make_error("extension.missing_capability", "Module lacks required capability"));
    }

    if (!descriptor.required_permission.empty() &&
        !permissions.contains(descriptor.required_permission)) {
        diagnostics.error("extension.missing_permission",
                          descriptor.module_id + " tried to register " + descriptor.id +
                              " without permission " + descriptor.required_permission);
        return std::unexpected(
            make_error("extension.missing_permission", "Module lacks required permission"));
    }

    extensions_.emplace(descriptor.id, std::move(descriptor));
    return {};
}

std::vector<ExtensionDescriptor> ExtensionRegistry::list() const {
    std::vector<ExtensionDescriptor> result;
    result.reserve(extensions_.size());
    for (const auto& [_, descriptor] : extensions_) {
        result.push_back(descriptor);
    }
    return result;
}

std::vector<ExtensionDescriptor> ExtensionRegistry::list(ExtensionKind kind) const {
    std::vector<ExtensionDescriptor> result;
    for (const auto& [_, descriptor] : extensions_) {
        if (descriptor.kind == kind) {
            result.push_back(descriptor);
        }
    }
    return result;
}

bool ExtensionRegistry::contains(std::string_view id) const {
    return extensions_.contains(std::string{id});
}

std::string extension_kind_to_string(ExtensionKind kind) {
    switch (kind) {
    case ExtensionKind::ServiceExtension:
        return "service_extension";
    case ExtensionKind::RuntimeCommandSource:
        return "runtime_command_source";
    case ExtensionKind::CompatibilityAdapter:
        return "compatibility_adapter";
    case ExtensionKind::VfsMountProvider:
        return "vfs_mount_provider";
    case ExtensionKind::ForeignAssetResolver:
        return "foreign_asset_resolver";
    case ExtensionKind::SaveExtensionStateProvider:
        return "save_extension_state_provider";
    case ExtensionKind::RuntimeEcsSystemPack:
        return "runtime_ecs_system_pack";
    case ExtensionKind::ScriptFunctionProvider:
        return "script_function_provider";
    case ExtensionKind::StoryGraphNodeProvider:
        return "story_graph_node_provider";
    case ExtensionKind::AssetValidator:
        return "asset_validator";
    case ExtensionKind::CookProcessor:
        return "cook_processor";
    case ExtensionKind::EditorPanelProvider:
        return "editor_panel_provider";
    case ExtensionKind::McpProvider:
        return "mcp_provider";
    case ExtensionKind::AIProvider:
        return "ai_provider";
    case ExtensionKind::VNPropertyType:
        return "vn_property_type";
    case ExtensionKind::PlatformProvider:
        return "platform_provider";
    case ExtensionKind::RendererProvider:
        return "renderer_provider";
    case ExtensionKind::AudioProvider:
        return "audio_provider";
    case ExtensionKind::ProjectContentProvider:
        return "project_content_provider";
    }
    return "unknown";
}

std::string default_capability_for_extension(ExtensionKind kind) {
    return extension_kind_to_string(kind);
}

} // namespace astra
