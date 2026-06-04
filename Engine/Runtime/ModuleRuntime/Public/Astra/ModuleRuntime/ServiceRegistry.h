#pragma once

#include <Astra/Core/Diagnostics.h>
#include <Astra/Core/Result.h>

#include <string>
#include <string_view>
#include <unordered_map>
#include <unordered_set>
#include <vector>

namespace astra {

struct ServiceDescriptor {
    std::string id;
    void* service = nullptr;
    std::string required_capability;
    std::string required_permission;
};

class ServiceRegistry {
  public:
    VoidResult register_service(ServiceDescriptor descriptor, DiagnosticSink& diagnostics);

    [[nodiscard]] void* resolve(std::string_view id, const std::unordered_set<std::string>& capabilities,
                                const std::unordered_set<std::string>& permissions,
                                DiagnosticSink& diagnostics) const;

    template <typename T>
    [[nodiscard]] T* resolve_typed(std::string_view id,
                                   const std::unordered_set<std::string>& capabilities,
                                   const std::unordered_set<std::string>& permissions,
                                   DiagnosticSink& diagnostics) const {
        return static_cast<T*>(resolve(id, capabilities, permissions, diagnostics));
    }

    [[nodiscard]] std::vector<ServiceDescriptor> list() const;
    [[nodiscard]] bool contains(std::string_view id) const;

  private:
    std::unordered_map<std::string, ServiceDescriptor> services_;
};

} // namespace astra
