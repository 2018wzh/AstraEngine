#pragma once

#include <Astra/Core/Diagnostics.h>
#include <Astra/Core/Result.h>

#include <cstdint>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

namespace astra {

enum class ExtensionKind : std::uint32_t {
    ServiceExtension = 0,
    PropertyTypeProvider = 1,
    EditorMetadataProvider = 2,
};

struct ExtensionDescriptor {
    std::string id;
    std::string module_id;
    ExtensionKind kind = ExtensionKind::ServiceExtension;
    std::string required_capability;
    std::string required_permission;
};

class ExtensionRegistry {
  public:
    VoidResult register_extension(ExtensionDescriptor descriptor,
                                  const std::unordered_set<std::string>& capabilities,
                                  const std::unordered_set<std::string>& permissions,
                                  DiagnosticSink& diagnostics);

    [[nodiscard]] std::vector<ExtensionDescriptor> list() const;
    [[nodiscard]] std::vector<ExtensionDescriptor> list(ExtensionKind kind) const;
    [[nodiscard]] bool contains(std::string_view id) const;

  private:
    std::unordered_map<std::string, ExtensionDescriptor> extensions_;
};

std::string extension_kind_to_string(ExtensionKind kind);
std::string default_capability_for_extension(ExtensionKind kind);

} // namespace astra
