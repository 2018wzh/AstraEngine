#pragma once

#include <Astra/Core/Diagnostics.h>
#include <Astra/Core/Result.h>
#include <Astra/VNRuntimeServices/RuntimeCommand.h>

#include <filesystem>
#include <string>
#include <unordered_map>
#include <vector>

namespace astra {

enum class ScriptInstructionKind {
    ShowBackground,
    ShowCharacter,
    PlayBGM,
    PlaySFX,
    Dialogue,
    Choice,
    SetVariable,
    JumpScene,
};

struct ScriptInstruction {
    ScriptInstructionKind kind = ScriptInstructionKind::Dialogue;
    std::string asset_id;
    std::string speaker;
    std::string text;
    std::string character_id;
    StagePosition position = StagePosition::Center;
    std::vector<ChoiceOption> choices;
    std::string variable;
    int delta = 0;
    std::string target_scene;
};

struct ScriptScene {
    std::string id;
    std::vector<ScriptInstruction> instructions;
};

struct AstraScript {
    std::string entry_scene;
    std::unordered_map<std::string, ScriptScene> scenes;
};

Expected<AstraScript> parse_astra_script(const std::filesystem::path& path,
                                         DiagnosticSink& diagnostics);

} // namespace astra
