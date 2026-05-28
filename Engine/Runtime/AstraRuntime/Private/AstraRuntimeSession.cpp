#include <Astra/AstraRuntime/AstraRuntimeSession.h>

#include <filesystem>

namespace astra {

namespace {

RuntimeCommand to_command(const ScriptInstruction& instruction) {
    switch (instruction.kind) {
    case ScriptInstructionKind::ShowBackground:
        return ShowBackground{AssetId{instruction.asset_id}, TransitionDesc{"cut", 0.0f}};
    case ScriptInstructionKind::ShowCharacter:
        return ShowCharacter{instruction.character_id, AssetId{instruction.asset_id},
                             instruction.position};
    case ScriptInstructionKind::PlayBGM:
        return PlayBGM{AssetId{instruction.asset_id}, 0.0f};
    case ScriptInstructionKind::PlaySFX:
        return PlaySFX{AssetId{instruction.asset_id}};
    case ScriptInstructionKind::Dialogue:
        return ShowDialogue{instruction.speaker, instruction.text};
    case ScriptInstructionKind::Choice:
        return PresentChoice{instruction.choices};
    case ScriptInstructionKind::SetVariable:
        return SetVariable{instruction.variable, instruction.delta};
    case ScriptInstructionKind::JumpScene:
        return JumpScene{instruction.target_scene};
    }
    return ShowDialogue{"system", "Unsupported command"};
}

std::string command_name(const ScriptInstruction& instruction) {
    switch (instruction.kind) {
    case ScriptInstructionKind::ShowBackground:
        return "ShowBackground " + instruction.asset_id;
    case ScriptInstructionKind::ShowCharacter:
        return "ShowCharacter " + instruction.asset_id;
    case ScriptInstructionKind::PlayBGM:
        return "PlayBGM " + instruction.asset_id;
    case ScriptInstructionKind::PlaySFX:
        return "PlaySFX " + instruction.asset_id;
    case ScriptInstructionKind::Dialogue:
        return "ShowDialogue " + instruction.speaker + ": " + instruction.text;
    case ScriptInstructionKind::Choice:
        return "PresentChoice";
    case ScriptInstructionKind::SetVariable:
        return "SetVariable " + instruction.variable;
    case ScriptInstructionKind::JumpScene:
        return "JumpScene " + instruction.target_scene;
    }
    return "Unknown";
}

} // namespace

AstraRuntimeSession::AstraRuntimeSession(RuntimeProviderRegistry& provider_registry,
                                         ExtensionRegistry& extension_registry)
    : provider_registry_(provider_registry), extension_registry_(extension_registry) {}

AstraRuntimeSession::~AstraRuntimeSession() = default;

VoidResult AstraRuntimeSession::load_project(const std::filesystem::path& project_root,
                                             DiagnosticSink& diagnostics) {
    auto provider = provider_registry_.project_content_provider();
    if (!provider) {
        diagnostics.error(provider.error().code, provider.error().message);
        return std::unexpected(provider.error());
    }
    auto loaded_content =
        (*provider)->load_project_content(project_root, vfs_, asset_registry_, diagnostics);
    if (!loaded_content) {
        return std::unexpected(loaded_content.error());
    }
    project_root_ = loaded_content->project_root;
    content_root_ = loaded_content->content_root;
    entry_script_ = loaded_content->entry_script;

    auto parsed = parse_astra_script(entry_script_, diagnostics);
    if (!parsed) {
        return std::unexpected(parsed.error());
    }
    script_ = std::move(*parsed);
    return {};
}

VoidResult AstraRuntimeSession::start(DiagnosticSink& diagnostics) {
    current_scene_ = script_.entry_scene;
    instruction_index_ = 0;
    waiting_for_choice_ = false;
    active_choices_.clear();
    command_log_.clear();
    return execute_until_blocked(diagnostics);
}

VoidResult AstraRuntimeSession::advance(DiagnosticSink& diagnostics) {
    if (waiting_for_choice_) {
        return {};
    }
    return execute_until_blocked(diagnostics);
}

VoidResult AstraRuntimeSession::choose(std::size_t choice_index, DiagnosticSink& diagnostics) {
    if (!waiting_for_choice_ || choice_index >= active_choices_.size()) {
        return std::unexpected(make_error("runtime.choice_invalid", "Choice index is not active"));
    }
    const ChoiceOption option = active_choices_[choice_index];
    waiting_for_choice_ = false;
    active_choices_.clear();
    if (!option.variable.empty()) {
        ScriptInstruction set;
        set.kind = ScriptInstructionKind::SetVariable;
        set.variable = option.variable;
        set.delta = option.delta;
        if (auto executed = execute_instruction(set, diagnostics); !executed) {
            return executed;
        }
    }
    if (!option.target_scene.empty()) {
        return jump_to(option.target_scene, diagnostics);
    }
    return execute_until_blocked(diagnostics);
}

void AstraRuntimeSession::tick() {
    services_.run_schedule();
}

RuntimeSaveSnapshot AstraRuntimeSession::save_snapshot() const {
    return services_.save_snapshot();
}

void AstraRuntimeSession::restore_snapshot(const RuntimeSaveSnapshot& snapshot) {
    services_.restore_snapshot(snapshot);
}

void AstraRuntimeSession::shutdown(DiagnosticSink& diagnostics) {
    (void)diagnostics;
}

RuntimeRenderSnapshot AstraRuntimeSession::render_snapshot() const {
    return services_.render_snapshot();
}

std::vector<RuntimeAudioRequest> AstraRuntimeSession::consume_audio_requests() {
    return services_.consume_audio_requests();
}

std::optional<std::filesystem::path>
AstraRuntimeSession::resolve_asset_source(std::string_view asset_id) const {
    auto id = AssetId::parse(std::string(asset_id));
    if (!id) {
        return std::nullopt;
    }
    auto metadata = asset_registry_.find(*id);
    if (!metadata) {
        return std::nullopt;
    }
    return content_root_ / metadata->source_path;
}

const std::vector<std::string>& AstraRuntimeSession::command_log() const {
    return command_log_;
}

const ExtensionRegistry& AstraRuntimeSession::extension_registry() const {
    return extension_registry_;
}

const AssetRegistry& AstraRuntimeSession::asset_registry() const {
    return asset_registry_;
}

VoidResult AstraRuntimeSession::execute_until_blocked(DiagnosticSink& diagnostics) {
    while (!waiting_for_choice_) {
        auto scene_it = script_.scenes.find(current_scene_);
        if (scene_it == script_.scenes.end()) {
            return std::unexpected(
                make_error("runtime.scene_missing", "Scene does not exist: " + current_scene_));
        }
        if (instruction_index_ >= scene_it->second.instructions.size()) {
            return {};
        }

        const ScriptInstruction instruction = scene_it->second.instructions[instruction_index_++];
        if (auto executed = execute_instruction(instruction, diagnostics); !executed) {
            return executed;
        }
        if (instruction.kind == ScriptInstructionKind::Dialogue ||
            instruction.kind == ScriptInstructionKind::Choice) {
            return {};
        }
    }
    return {};
}

VoidResult AstraRuntimeSession::execute_instruction(const ScriptInstruction& instruction,
                                                    DiagnosticSink& diagnostics) {
    if (instruction.kind == ScriptInstructionKind::JumpScene) {
        return jump_to(instruction.target_scene, diagnostics);
    }
    RuntimeCommand command = to_command(instruction);
    services_.submit_command(command, diagnostics);
    log_command(command_name(instruction));
    if (instruction.kind == ScriptInstructionKind::Choice) {
        waiting_for_choice_ = true;
        active_choices_ = instruction.choices;
    }
    services_.run_schedule();
    return {};
}

VoidResult AstraRuntimeSession::jump_to(std::string scene_id, DiagnosticSink& diagnostics) {
    if (!script_.scenes.contains(scene_id)) {
        diagnostics.error("runtime.scene_missing", "Scene does not exist: " + scene_id);
        return std::unexpected(make_error("runtime.scene_missing", "Scene does not exist"));
    }
    current_scene_ = std::move(scene_id);
    instruction_index_ = 0;
    log_command("JumpScene " + current_scene_);
    return execute_until_blocked(diagnostics);
}

void AstraRuntimeSession::log_command(std::string command) {
    command_log_.push_back(std::move(command));
}

} // namespace astra
