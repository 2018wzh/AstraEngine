#include <Astra/ModuleRuntime/ExtensionRegistry.h>

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
    return extensions_.contains(std::string(id));
}

std::string extension_kind_to_string(ExtensionKind kind) {
    switch (kind) {
    case ExtensionKind::ServiceExtension:
        return "service_extension";
    case ExtensionKind::PropertyTypeProvider:
        return "property_type_provider";
    case ExtensionKind::EditorMetadataProvider:
        return "editor_metadata_provider";
    }
    return "service_extension";
}

std::string default_capability_for_extension(ExtensionKind kind) {
    return extension_kind_to_string(kind);
}

} // namespace astra
