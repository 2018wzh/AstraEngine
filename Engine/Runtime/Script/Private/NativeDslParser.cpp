#include "ScriptPrivate.hpp"

#include <Astra/Core/Logging.hpp>

#include <tao/pegtl.hpp>

#include <algorithm>
#include <cctype>
#include <optional>
#include <sstream>
#include <unordered_map>

namespace Astra::Script {

namespace {
namespace pegtl = tao::pegtl;

struct IdentifierFirst : pegtl::sor<pegtl::alpha, pegtl::one<'_'>> {};
struct IdentifierRest : pegtl::sor<pegtl::alnum, pegtl::one<'_', '-', '.', '/'>> {};
struct IdentifierGrammar : pegtl::seq<IdentifierFirst, pegtl::star<IdentifierRest>, pegtl::eof> {};
struct DurationGrammar : pegtl::seq<pegtl::plus<pegtl::digit>, pegtl::sor<pegtl::string<'m', 's'>, pegtl::one<'s'>>, pegtl::eof> {};
struct StableIdGrammar : pegtl::seq<pegtl::plus<IdentifierRest>, pegtl::eof> {};

bool PegtlMatchesIdentifier(std::string_view value) {
    pegtl::memory_input input(value.data(), value.size(), "identifier");
    return pegtl::parse<IdentifierGrammar>(input);
}

bool PegtlMatchesDuration(std::string_view value) {
    pegtl::memory_input input(value.data(), value.size(), "duration");
    return pegtl::parse<DurationGrammar>(input);
}

bool PegtlMatchesStableId(std::string_view value) {
    pegtl::memory_input input(value.data(), value.size(), "stable_id");
    return pegtl::parse<StableIdGrammar>(input);
}

std::size_t IndentOf(std::string_view line) {
    std::size_t indent = 0;
    while (indent < line.size() && line[indent] == ' ') {
        ++indent;
    }
    return indent;
}

std::optional<std::string> ExtractStableId(std::string_view line) {
    const auto marker = line.find("#@id");
    if (marker == std::string_view::npos) {
        return std::nullopt;
    }
    auto id = Private::Trim(line.substr(marker + 4));
    if (const auto space = id.find(' '); space != std::string::npos) {
        id = id.substr(0, space);
    }
    if (id.empty() || !PegtlMatchesStableId(id)) {
        return std::nullopt;
    }
    return id;
}

std::string RemoveStableIdComment(std::string_view line) {
    const auto marker = line.find("#@id");
    return Private::Trim(marker == std::string_view::npos ? line : line.substr(0, marker));
}

std::optional<std::string> Quoted(std::string_view text) {
    const auto first = text.find('"');
    if (first == std::string_view::npos) {
        return std::nullopt;
    }
    const auto second = text.find('"', first + 1);
    if (second == std::string_view::npos) {
        return std::nullopt;
    }
    return std::string(text.substr(first + 1, second - first - 1));
}

std::optional<std::pair<std::string, std::string>> QuotedRoute(std::string_view text) {
    const auto choice = Quoted(text);
    if (!choice) {
        return std::nullopt;
    }
    const auto arrow = text.find("->");
    if (arrow == std::string_view::npos) {
        return std::nullopt;
    }
    return std::pair{*choice, Private::Trim(text.substr(arrow + 2))};
}

bool NeedsStableId(ScriptCommandKind kind) {
    switch (kind) {
    case ScriptCommandKind::Story:
    case ScriptCommandKind::State:
    case ScriptCommandKind::Label:
    case ScriptCommandKind::Sequence:
    case ScriptCommandKind::Parallel:
    case ScriptCommandKind::Race:
        return false;
    default:
        return true;
    }
}

bool SetAsset(ScriptCommand& command, std::string_view asset_text, Astra::Core::DiagnosticSink& diagnostics) {
    auto asset = Astra::Asset::ParseAssetUri(asset_text);
    if (!asset) {
        Private::EmitBlocking(diagnostics, command.location, "ASTRA_SCRIPT_ASSET_INVALID", "Script command references an invalid asset URI.", "Use a stable URI such as native:/Backgrounds/Room.");
        return false;
    }
    command.asset = asset.Value();
    return true;
}

nlohmann::json NamedArgs(const std::vector<std::string>& words, std::size_t begin) {
    nlohmann::json args = nlohmann::json::object();
    for (std::size_t index = begin; index < words.size(); ++index) {
        const auto colon = words[index].find(':');
        if (colon != std::string::npos) {
            args[words[index].substr(0, colon)] = words[index].substr(colon + 1);
        }
    }
    return args;
}

void RequireStableId(ScriptCommand& command, std::string_view original_line, Astra::Core::DiagnosticSink& diagnostics) {
    command.stable_id = ExtractStableId(original_line).value_or("");
    if (NeedsStableId(command.kind) && command.stable_id.empty()) {
        Private::EmitBlocking(diagnostics, command.location, "ASTRA_SCRIPT_STABLE_ID_REQUIRED", "Production .astra command requires a stable #@id.", "Add a stable marker such as #@id line_opening_001.");
    }
}

bool EndsScene(const ScriptCommand& command) {
    return command.kind == ScriptCommandKind::Jump || command.kind == ScriptCommandKind::Return || command.kind == ScriptCommandKind::End || command.kind == ScriptCommandKind::Await;
}

} // namespace

Astra::Core::Result<CompiledScript> ScriptRuntimeHost::CompileNative(const ScriptSource& source, Astra::Core::DiagnosticSink& diagnostics) const {
    Astra::Core::DefaultLogger().Log(
        "script.compile",
        NativeRuntimeId,
        Astra::Core::LogLevel::Info,
        "phase8 native script compile started",
        {{"source", source.source_id}, {"file", source.file}});

    CompiledScript script;
    script.runtime_id = NativeRuntimeId;
    script.source_id = source.source_id;
    script.document.source_id = source.source_id;

    std::istringstream input(source.text);
    std::string line;
    Astra::Core::u32 line_number = 0;
    std::string current_story;
    std::string current_state;
    std::string current_scene;
    bool scene_open = false;
    bool scene_has_terminal = false;

    auto append = [&](ScriptCommand command) {
        command.story = current_story;
        command.state = current_state;
        command.scene = current_scene;
        if (EndsScene(command)) {
            scene_has_terminal = true;
        }
        Private::AppendCommand(script, std::move(command), diagnostics);
    };

    while (std::getline(input, line)) {
        ++line_number;
        const auto trimmed_original = Private::Trim(line);
        if (trimmed_original.empty() || Private::StartsWith(trimmed_original, "#")) {
            continue;
        }
        const auto content = RemoveStableIdComment(trimmed_original);
        const auto words = Private::SplitWords(content);
        if (words.empty()) {
            continue;
        }

        auto make = [&](ScriptCommandKind kind) {
            auto command = Private::MakeCommand(kind, source, line_number, static_cast<Astra::Core::u32>(script.commands.size()));
            RequireStableId(command, trimmed_original, diagnostics);
            return command;
        };

        if (words[0] == "story" && words.size() >= 2) {
            current_story = words[1].ends_with(':') ? words[1].substr(0, words[1].size() - 1) : words[1];
            if (!PegtlMatchesIdentifier(current_story)) {
                Private::EmitBlocking(diagnostics, {source.file, line_number, 1}, "ASTRA_SCRIPT_IDENTIFIER_INVALID", "story name is not a valid identifier.", "Use letters, numbers, _, -, . or /.");
            }
            auto command = make(ScriptCommandKind::Story);
            command.story = current_story;
            append(std::move(command));
        } else if (words[0] == "state" && words.size() >= 2) {
            current_state = words[1].ends_with(':') ? words[1].substr(0, words[1].size() - 1) : words[1];
            auto command = make(ScriptCommandKind::State);
            command.state = current_state;
            append(std::move(command));
        } else if (words[0] == "initial" || words[0] == "enter:" || words[0] == "exit:" || words[0] == "data:" || words[0] == "project:") {
            script.document.cst["declarations"].push_back({{"line", line_number}, {"text", content}});
        } else if (words[0] == "scene" && words.size() >= 2) {
            if (scene_open && !scene_has_terminal) {
                Private::EmitBlocking(diagnostics, {source.file, line_number, 1}, "ASTRA_SCRIPT_SCENE_FALLTHROUGH", "Scene must end with ->, return, end, or await before the next scene.", "Add an explicit scene terminator.");
            }
            current_scene = words[1].ends_with(':') ? words[1].substr(0, words[1].size() - 1) : words[1];
            scene_open = true;
            scene_has_terminal = false;
            auto command = make(ScriptCommandKind::Scene);
            command.scene = current_scene;
            command.label = current_scene;
            append(std::move(command));
        } else if (words[0] == "stage:") {
            auto command = make(ScriptCommandKind::Stage);
            command.payload = {{"kind", "stage_snapshot"}};
            append(std::move(command));
        } else if (words[0] == "timeline" && words.size() >= 2) {
            if (!PegtlMatchesDuration(words[1].ends_with(':') ? std::string_view(words[1]).substr(0, words[1].size() - 1) : std::string_view(words[1]))) {
                Private::EmitBlocking(diagnostics, {source.file, line_number, 1}, "ASTRA_SCRIPT_DURATION_INVALID", "timeline requires a duration with units.", "Use timeline 900ms: or timeline 1s:");
            }
            auto command = make(ScriptCommandKind::Timeline);
            command.payload = {{"duration", words[1]}, {"tracks", nlohmann::json::array()}};
            append(std::move(command));
        } else if (words[0] == "background" || words[0] == "bg") {
            auto command = make(ScriptCommandKind::Background);
            if (words.size() >= 2 && SetAsset(command, words[1], diagnostics)) {
                command.payload = NamedArgs(words, 2);
                append(std::move(command));
            }
        } else if (words[0] == "show" && words.size() >= 3) {
            auto command = make(ScriptCommandKind::ShowCharacter);
            command.actor = words[1];
            command.placement = "center";
            auto asset_index = std::size_t{2};
            if (words[2].starts_with("pose:")) {
                command.payload["pose"] = words[2].substr(5);
                asset_index = 3;
            }
            if (asset_index < words.size() && SetAsset(command, words[asset_index], diagnostics)) {
                for (const auto& word : words) {
                    if (word.starts_with("at:")) {
                        command.placement = word.substr(3);
                    }
                }
                append(std::move(command));
            }
        } else if (words[0] == "pose" && words.size() >= 3) {
            auto command = make(ScriptCommandKind::Pose);
            command.actor = words[1];
            command.payload = {{"pose", words[2]}};
            append(std::move(command));
        } else if (words[0] == "move" && words.size() >= 3) {
            auto command = make(ScriptCommandKind::Move);
            command.actor = words[1];
            command.placement = words[2].starts_with("to:") ? words[2].substr(3) : words[2];
            command.payload = NamedArgs(words, 2);
            append(std::move(command));
        } else if (words[0] == "camera") {
            auto command = make(ScriptCommandKind::Camera);
            command.payload = {{"raw", content}, {"args", NamedArgs(words, 1)}};
            append(std::move(command));
        } else if (words[0] == "play" && words.size() >= 3) {
            auto command = make(ScriptCommandKind::Audio);
            command.payload = {{"bus", words[1]}, {"args", NamedArgs(words, 3)}};
            if (SetAsset(command, words[2], diagnostics)) {
                append(std::move(command));
            }
        } else if (words[0] == "filter" && words.size() >= 2) {
            auto command = make(ScriptCommandKind::Filter);
            if (SetAsset(command, words[1], diagnostics)) {
                append(std::move(command));
            }
        } else if ((content.find(':') != std::string::npos && words[0] != "choice" && !words[0].starts_with("@")) || words[0] == "say") {
            auto command = make(ScriptCommandKind::Say);
            if (words[0] == "say") {
                command.actor = words.size() >= 2 ? words[1] : "narrator";
                command.text = Quoted(content).value_or("");
            } else {
                const auto colon = content.find(':');
                command.actor = content.substr(0, colon);
                if (const auto bracket = command.actor.find('['); bracket != std::string::npos) {
                    command.payload["pose"] = command.actor.substr(bracket + 1, command.actor.find(']') - bracket - 1);
                    command.actor = command.actor.substr(0, bracket);
                }
                command.text = Private::Trim(content.substr(colon + 1));
                if (command.text.starts_with('"') && command.text.ends_with('"')) {
                    command.text = command.text.substr(1, command.text.size() - 2);
                }
            }
            if (command.text.empty()) {
                Private::EmitBlocking(diagnostics, command.location, "ASTRA_SCRIPT_DIALOGUE_EMPTY", "dialogue text cannot be empty.", "Add text after the speaker colon.");
            }
            append(std::move(command));
        } else if (words[0] == "choice") {
            auto command = make(ScriptCommandKind::Choice);
            command.text = Quoted(content).value_or("");
            const auto base_indent = IndentOf(line);
            std::streampos before_choice_entry;
            while (true) {
                before_choice_entry = input.tellg();
                std::string choice_line;
                if (!std::getline(input, choice_line)) {
                    break;
                }
                ++line_number;
                if (Private::Trim(choice_line).empty()) {
                    continue;
                }
                if (IndentOf(choice_line) <= base_indent) {
                    input.seekg(before_choice_entry);
                    --line_number;
                    break;
                }
                const auto choice_clean = RemoveStableIdComment(Private::Trim(choice_line));
                if (!choice_clean.starts_with("- ")) {
                    continue;
                }
                auto route = QuotedRoute(choice_clean);
                if (!route) {
                    Private::EmitBlocking(diagnostics, {source.file, line_number, 1}, "ASTRA_SCRIPT_CHOICE_INVALID", "choice entry requires quoted text and -> target.", "Use - \"Text\" -> target_scene.");
                    continue;
                }
                auto choice_id = ExtractStableId(choice_line).value_or("");
                if (choice_id.empty()) {
                    Private::EmitBlocking(diagnostics, {source.file, line_number, 1}, "ASTRA_SCRIPT_STABLE_ID_REQUIRED", "choice entry requires #@id.", "Add #@id choice_option_name.");
                }
                command.choices.push_back({choice_id, route->first, route->second});
            }
            if (command.choices.empty()) {
                Private::EmitBlocking(diagnostics, command.location, "ASTRA_SCRIPT_CHOICE_EMPTY", "choice requires at least one option.", "Add indented - \"Text\" -> scene entries.");
            }
            append(std::move(command));
        } else if (words[0] == "->" && words.size() >= 2) {
            auto command = make(ScriptCommandKind::Jump);
            command.target_label = words[1];
            append(std::move(command));
        } else if (words[0] == "return") {
            append(make(ScriptCommandKind::Return));
        } else if (words[0] == "end") {
            append(make(ScriptCommandKind::End));
        } else if (words[0] == "await" && words.size() >= 2) {
            auto command = make(ScriptCommandKind::Await);
            command.target_label = current_scene;
            command.payload = {{"event", words[1]}};
            append(std::move(command));
        } else if (words[0] == "set" && words.size() >= 3) {
            auto command = make(ScriptCommandKind::SetVariable);
            command.variable_name = words[1];
            command.value = words[2] == "=" && words.size() >= 4 ? words[3] : words[2];
            append(std::move(command));
        } else if (words[0] == "inc" && words.size() >= 2) {
            auto command = make(ScriptCommandKind::IncrementVariable);
            command.variable_name = words[1];
            command.value = 1;
            append(std::move(command));
        } else if (words[0].starts_with("@")) {
            auto command = make(ScriptCommandKind::Extension);
            const auto dot = words[0].find('.');
            if (dot == std::string::npos) {
                Private::EmitBlocking(diagnostics, command.location, "ASTRA_SCRIPT_EXTENSION_INVALID", "extension command must use @extension.command syntax.", "Use @live2d.motion.play actor:alice.");
            } else {
                command.extension_id = words[0].substr(1, dot - 1);
                command.extension_command = words[0].substr(dot + 1);
                command.payload = {{"args", NamedArgs(words, 1)}, {"schema_required", true}, {"channels", nlohmann::json::array({command.extension_id + "." + command.extension_command})}};
                script.extension_manifest.push_back({{"extension", command.extension_id}, {"command", command.extension_command}, {"stable_id", command.stable_id}});
            }
            append(std::move(command));
        } else if (words[0] == "sequence:" || words[0] == "parallel:" || words[0] == "race:") {
            auto kind = words[0] == "parallel:" ? ScriptCommandKind::Parallel : words[0] == "race:" ? ScriptCommandKind::Race : ScriptCommandKind::Sequence;
            append(make(kind));
        } else {
            Private::EmitBlocking(diagnostics, {source.file, line_number, 1}, "ASTRA_SCRIPT_COMMAND_UNKNOWN", "Unknown Phase 8 .astra command.", "Use the production .astra DSL commands documented in Native DSL Reference.");
        }
    }

    if (scene_open && !scene_has_terminal) {
        Private::EmitBlocking(diagnostics, {source.file, line_number, 1}, "ASTRA_SCRIPT_SCENE_FALLTHROUGH", "Last scene must end with ->, return, end, or await.", "Add end #@id end_scene_name.");
    }

    if (!Private::ValidateCompiledScript(script, diagnostics)) {
        return Astra::Core::Result<CompiledScript>::Failure(Astra::Core::ErrorCode::InvalidFormat, "phase8 native script compilation failed");
    }

    return Astra::Core::Result<CompiledScript>::Success(std::move(script));
}

} // namespace Astra::Script
