#pragma once

#include <Astra/AssetCore/AssetId.h>

#include <nlohmann/json.hpp>

#include <string>
#include <unordered_map>
#include <variant>
#include <vector>

namespace astra {

enum class StagePosition {
    Left,
    Center,
    Right,
};

struct TransitionDesc {
    std::string kind = "cut";
    float duration_seconds = 0.0f;
};

struct ShowBackground {
    AssetId background;
    TransitionDesc transition;
};

struct ShowCharacter {
    std::string character_id;
    AssetId sprite;
    StagePosition position = StagePosition::Center;
};

struct PlayBGM {
    AssetId bgm;
    float fade_in_seconds = 0.0f;
};

struct PlaySFX {
    AssetId sfx;
};

struct ShowDialogue {
    std::string speaker;
    std::string text;
};

struct ChoiceOption {
    std::string text;
    std::string target_scene;
    std::string variable;
    int delta = 0;
};

struct PresentChoice {
    std::vector<ChoiceOption> options;
};

struct SetVariable {
    std::string name;
    int delta = 0;
};

struct JumpScene {
    std::string target_scene;
};

using RuntimeCommand = std::variant<ShowBackground, ShowCharacter, PlayBGM, PlaySFX, ShowDialogue,
                                    PresentChoice, SetVariable, JumpScene>;

struct RenderSprite {
    std::string asset_id;
    StagePosition position = StagePosition::Center;
};

struct RuntimeRenderSnapshot {
    std::string background_asset;
    std::vector<RenderSprite> sprites;
    std::string speaker;
    std::string dialogue;
    std::vector<std::string> choices;
    std::unordered_map<std::string, int> variables;
};

struct RuntimeAudioRequest {
    std::string asset_id;
    bool loop = false;
};

struct RuntimeSaveSnapshot {
    nlohmann::json world;
};

} // namespace astra
