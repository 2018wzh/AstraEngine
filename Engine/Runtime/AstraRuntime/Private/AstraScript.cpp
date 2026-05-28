#include <Astra/AstraRuntime/AstraScript.h>

#include <Astra/AssetCore/AssetId.h>
#include <Astra/Core/Path.h>

#include <fstream>
#include <regex>
#include <sstream>

namespace astra {

namespace {

std::string trim(std::string value) {
    const auto begin = value.find_first_not_of(" \t\r\n");
    if (begin == std::string::npos) {
        return {};
    }
    const auto end = value.find_last_not_of(" \t\r\n");
    return value.substr(begin, end - begin + 1);
}

bool starts_with(std::string_view value, std::string_view prefix) {
    return value.substr(0, prefix.size()) == prefix;
}

std::string strip_quotes(std::string value) {
    value = trim(std::move(value));
    if (value.size() >= 2 && value.front() == '"' && value.back() == '"') {
        return value.substr(1, value.size() - 2);
    }
    return value;
}

StagePosition parse_position(std::string_view value) {
    if (value == "left") {
        return StagePosition::Left;
    }
    if (value == "right") {
        return StagePosition::Right;
    }
    return StagePosition::Center;
}

Expected<AssetId> parse_asset(std::string value) {
    return AssetId::parse(strip_quotes(std::move(value)));
}

} // namespace

Expected<AstraScript> parse_astra_script(const std::filesystem::path& path,
                                         DiagnosticSink& diagnostics) {
    std::ifstream file(path);
    if (!file) {
        return std::unexpected(
            make_error("script.open_failed", "Cannot open script: " + path_to_utf8(path)));
    }

    std::vector<std::string> lines;
    std::string line;
    while (std::getline(file, line)) {
        lines.push_back(line);
    }

    AstraScript script;
    ScriptScene* current_scene = nullptr;

    for (std::size_t i = 0; i < lines.size(); ++i) {
        std::string raw = lines[i];
        std::string text = trim(raw);
        if (text.empty() || starts_with(text, "#")) {
            continue;
        }

        if (starts_with(text, "scene ") && text.ends_with(":")) {
            const std::string scene_id = trim(text.substr(6, text.size() - 7));
            auto [it, _] = script.scenes.emplace(scene_id, ScriptScene{scene_id, {}});
            current_scene = &it->second;
            if (script.entry_scene.empty()) {
                script.entry_scene = scene_id;
            }
            continue;
        }

        if (current_scene == nullptr) {
            diagnostics.error("script.instruction_without_scene",
                              "Instruction appears before a scene at line " +
                                  std::to_string(i + 1));
            continue;
        }

        if (starts_with(text, "agent ")) {
            diagnostics.warning("script.unsupported_agent",
                                "agent blocks are not supported in runtime 0.1 at line " +
                                    std::to_string(i + 1));
            while (i + 1 < lines.size() && starts_with(lines[i + 1], "    ")) {
                ++i;
            }
            continue;
        }

        std::smatch match;
        if (std::regex_match(text, match, std::regex(R"(^bg\s+("[^"]+")(?:\s+with\s+.*)?$)"))) {
            auto id = parse_asset(match[1].str());
            if (!id) {
                diagnostics.error(id.error().code, id.error().message);
                continue;
            }
            ScriptInstruction instruction;
            instruction.kind = ScriptInstructionKind::ShowBackground;
            instruction.asset_id = id->value();
            current_scene->instructions.push_back(std::move(instruction));
            continue;
        }

        if (std::regex_match(text, match,
                             std::regex(R"(^show\s+("[^"]+")(?:\s+at\s+([a-zA-Z_]+))?$)"))) {
            auto id = parse_asset(match[1].str());
            if (!id) {
                diagnostics.error(id.error().code, id.error().message);
                continue;
            }
            ScriptInstruction instruction;
            instruction.kind = ScriptInstructionKind::ShowCharacter;
            instruction.asset_id = id->value();
            instruction.character_id = "character";
            instruction.position = parse_position(match[2].str());
            current_scene->instructions.push_back(std::move(instruction));
            continue;
        }

        if (std::regex_match(text, match, std::regex(R"(^play\s+bgm\s+("[^"]+")$)"))) {
            auto id = parse_asset(match[1].str());
            if (!id) {
                diagnostics.error(id.error().code, id.error().message);
                continue;
            }
            ScriptInstruction instruction;
            instruction.kind = ScriptInstructionKind::PlayBGM;
            instruction.asset_id = id->value();
            current_scene->instructions.push_back(std::move(instruction));
            continue;
        }

        if (std::regex_match(text, match, std::regex(R"(^play\s+sfx\s+("[^"]+")$)"))) {
            auto id = parse_asset(match[1].str());
            if (!id) {
                diagnostics.error(id.error().code, id.error().message);
                continue;
            }
            ScriptInstruction instruction;
            instruction.kind = ScriptInstructionKind::PlaySFX;
            instruction.asset_id = id->value();
            current_scene->instructions.push_back(std::move(instruction));
            continue;
        }

        if (starts_with(text, "choice:")) {
            ScriptInstruction choice;
            choice.kind = ScriptInstructionKind::Choice;
            while (i + 1 < lines.size()) {
                if (trim(lines[i + 1]).empty()) {
                    ++i;
                    continue;
                }
                if (!starts_with(lines[i + 1], "    ") || starts_with(lines[i + 1], "      ")) {
                    break;
                }
                ++i;
                std::string option_line = trim(lines[i]);
                if (!option_line.ends_with(":")) {
                    diagnostics.error("script.invalid_choice_option",
                                      "Expected choice option at line " + std::to_string(i + 1));
                    continue;
                }
                ChoiceOption option;
                option.text = strip_quotes(option_line.substr(0, option_line.size() - 1));
                while (i + 1 < lines.size()) {
                    if (trim(lines[i + 1]).empty()) {
                        ++i;
                        break;
                    }
                    if (!starts_with(lines[i + 1], "      ")) {
                        break;
                    }
                    ++i;
                    std::string action = trim(lines[i]);
                    if (std::regex_match(
                            action, match,
                            std::regex(R"(^set\s+([a-zA-Z0-9_.-]+)\s*\+=\s*(-?\d+)$)"))) {
                        option.variable = match[1].str();
                        option.delta = std::stoi(match[2].str());
                    } else if (std::regex_match(action, match,
                                                std::regex(R"(^goto\s+([a-zA-Z0-9_.-]+)$)"))) {
                        option.target_scene = match[1].str();
                    } else {
                        diagnostics.error("script.invalid_choice_action",
                                          "Unsupported choice action at line " +
                                              std::to_string(i + 1));
                    }
                }
                choice.choices.push_back(std::move(option));
            }
            current_scene->instructions.push_back(std::move(choice));
            continue;
        }

        if (std::regex_match(text, match,
                             std::regex(R"(^set\s+([a-zA-Z0-9_.-]+)\s*\+=\s*(-?\d+)$)"))) {
            ScriptInstruction instruction;
            instruction.kind = ScriptInstructionKind::SetVariable;
            instruction.variable = match[1].str();
            instruction.delta = std::stoi(match[2].str());
            current_scene->instructions.push_back(std::move(instruction));
            continue;
        }

        if (std::regex_match(text, match, std::regex(R"(^goto\s+([a-zA-Z0-9_.-]+)$)"))) {
            ScriptInstruction instruction;
            instruction.kind = ScriptInstructionKind::JumpScene;
            instruction.target_scene = match[1].str();
            current_scene->instructions.push_back(std::move(instruction));
            continue;
        }

        if (std::regex_match(text, match,
                             std::regex(R"ASTRA(^([a-zA-Z0-9_.-]+)\s+"([^"]*)"$)ASTRA"))) {
            ScriptInstruction instruction;
            instruction.kind = ScriptInstructionKind::Dialogue;
            instruction.speaker = match[1].str();
            instruction.text = match[2].str();
            current_scene->instructions.push_back(std::move(instruction));
            continue;
        }

        diagnostics.error("script.unsupported_instruction",
                          "Unsupported instruction at line " + std::to_string(i + 1) + ": " + text);
    }

    if (script.scenes.empty()) {
        return std::unexpected(make_error("script.no_scenes", "Script contains no scenes"));
    }
    if (diagnostics.has_errors()) {
        return std::unexpected(make_error("script.parse_failed", "Script parse failed"));
    }
    return script;
}

} // namespace astra
