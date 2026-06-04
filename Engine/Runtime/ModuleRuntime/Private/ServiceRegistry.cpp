#include <Astra/ModuleRuntime/ServiceRegistry.h>

namespace astra {

VoidResult ServiceRegistry::register_service(ServiceDescriptor descriptor,
                                             DiagnosticSink& diagnostics) {
    if (descriptor.id.empty()) {
        diagnostics.error("service.empty_id", "Service id must not be empty");
        return std::unexpected(make_error("service.empty_id", "Service id must not be empty"));
    }
    if (descriptor.service == nullptr) {
        diagnostics.error("service.null_service", "Service pointer must not be null");
        return std::unexpected(make_error("service.null_service", "Service pointer must not be null"));
    }
    if (services_.contains(descriptor.id)) {
        diagnostics.error("service.duplicate", "Duplicate service id: " + descriptor.id);
        return std::unexpected(make_error("service.duplicate", "Duplicate service id"));
    }
    services_.emplace(descriptor.id, std::move(descriptor));
    return {};
}

void* ServiceRegistry::resolve(std::string_view id,
                               const std::unordered_set<std::string>& capabilities,
                               const std::unordered_set<std::string>& permissions,
                               DiagnosticSink& diagnostics) const {
    const auto it = services_.find(std::string(id));
    if (it == services_.end()) {
        diagnostics.error("service.not_found", "Unknown service id: " + std::string(id));
        return nullptr;
    }
    const ServiceDescriptor& descriptor = it->second;
    if (!descriptor.required_capability.empty() &&
        !capabilities.contains(descriptor.required_capability)) {
        diagnostics.error("service.missing_capability",
                          std::string(id) + " requires capability " + descriptor.required_capability);
        return nullptr;
    }
    if (!descriptor.required_permission.empty() &&
        !permissions.contains(descriptor.required_permission)) {
        diagnostics.error("service.missing_permission",
                          std::string(id) + " requires permission " + descriptor.required_permission);
        return nullptr;
    }
    return descriptor.service;
}

std::vector<ServiceDescriptor> ServiceRegistry::list() const {
    std::vector<ServiceDescriptor> result;
    result.reserve(services_.size());
    for (const auto& [_, descriptor] : services_) {
        result.push_back(descriptor);
    }
    return result;
}

bool ServiceRegistry::contains(std::string_view id) const {
    return services_.contains(std::string(id));
}

} // namespace astra
