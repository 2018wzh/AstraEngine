#pragma once

#include <Astra/AssetRegistry/AssetRegistry.h>
#include <Astra/AstraRuntime/AstraScript.h>
#include <Astra/Bootstrap/RuntimeProviderRegistry.h>
#include <Astra/ExtensionRegistry/ExtensionRegistry.h>
#include <Astra/VFS/DirectoryMount.h>
#include <Astra/VNRuntimeServices/RuntimeServices.h>

#include <filesystem>
#include <optional>
#include <string>
#include <vector>

namespace astra {

class AstraRuntimeSession {
  public:
    AstraRuntimeSession(RuntimeProviderRegistry& provider_registry,
                        ExtensionRegistry& extension_registry);
    ~AstraRuntimeSession();

    VoidResult load_project(const std::filesystem::path& project_root,
                            DiagnosticSink& diagnostics);
    VoidResult start(DiagnosticSink& diagnostics);
    VoidResult advance(DiagnosticSink& diagnostics);
    VoidResult choose(std::size_t choice_index, DiagnosticSink& diagnostics);
    void tick();
    [[nodiscard]] RuntimeSaveSnapshot save_snapshot() const;
    void restore_snapshot(const RuntimeSaveSnapshot& snapshot);
    void shutdown(DiagnosticSink& diagnostics);

    [[nodiscard]] RuntimeRenderSnapshot render_snapshot() const;
    [[nodiscard]] std::vector<RuntimeAudioRequest> consume_audio_requests();
    [[nodiscard]] std::optional<std::filesystem::path>
    resolve_asset_source(std::string_view asset_id) const;
    [[nodiscard]] const std::vector<std::string>& command_log() const;
    [[nodiscard]] const ExtensionRegistry& extension_registry() const;
    [[nodiscard]] const AssetRegistry& asset_registry() const;

  private:
    VoidResult execute_until_blocked(DiagnosticSink& diagnostics);
    VoidResult execute_instruction(const ScriptInstruction& instruction,
                                   DiagnosticSink& diagnostics);
    VoidResult jump_to(std::string scene_id, DiagnosticSink& diagnostics);
    void log_command(std::string command);

    RuntimeProviderRegistry& provider_registry_;
    ExtensionRegistry& extension_registry_;
    std::filesystem::path project_root_;
    std::filesystem::path content_root_;
    std::filesystem::path entry_script_;
    Vfs vfs_;
    AssetRegistry asset_registry_;
    RuntimeServices services_;
    AstraScript script_;
    std::string current_scene_;
    std::size_t instruction_index_ = 0;
    bool waiting_for_choice_ = false;
    std::vector<ChoiceOption> active_choices_;
    std::vector<std::string> command_log_;
};

} // namespace astra
