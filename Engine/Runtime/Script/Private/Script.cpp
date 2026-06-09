#include <Astra/Script/Script.hpp>

#include <sol/sol.hpp>

#include <algorithm>
#include <cctype>
#include <sstream>
#include <unordered_set>

namespace Astra::Script {

namespace {

std::string Trim(std::string_view text) {
    auto begin = text.begin();
    auto end = text.end();
    while (begin != end && std::isspace(static_cast<unsigned char>(*begin)) != 0) {
        ++begin;
    }
    while (end != begin && std::isspace(static_cast<unsigned char>(*(end - 1))) != 0) {
        --end;
    }
    return std::string(begin, end);
}

bool StartsWith(std::string_view text, std::string_view prefix) {
    return text.size() >= prefix.size() && text.substr(0, prefix.size()) == prefix;
}

std::vector<std::string> SplitWords(std::string_view text) {
    std::istringstream input{std::string(text)};
    std::vector<std::string> words;
    std::string word;
    while (input >> word) {
        words.push_back(word);
    }
    return words;
}

std::optional<std::pair<std::string, std::string>> ParseQuotedRoute(std::string_view text) {
    const auto first_quote = text.find('"');
    if (first_quote == std::string_view::npos) {
        return std::nullopt;
    }
    const auto second_quote = text.find('"', first_quote + 1);
    if (second_quote == std::string_view::npos) {
        return std::nullopt;
    }
    const auto arrow = text.find("->", second_quote + 1);
    if (arrow == std::string_view::npos) {
        return std::nullopt;
    }
    return std::pair{
        std::string(text.substr(first_quote + 1, second_quote - first_quote - 1)),
        Trim(text.substr(arrow + 2)),
    };
}

std::optional<std::string> ParseQuotedText(std::string_view text) {
    const auto first_quote = text.find('"');
    if (first_quote == std::string_view::npos) {
        return std::nullopt;
    }
    const auto second_quote = text.find('"', first_quote + 1);
    if (second_quote == std::string_view::npos) {
        return std::nullopt;
    }
    return std::string(text.substr(first_quote + 1, second_quote - first_quote - 1));
}

Astra::Core::Diagnostic MakeDiagnostic(
    std::string code,
    Astra::Core::DiagnosticSeverity severity,
    std::string message,
    const ScriptSourceLocation& location,
    std::string fix = {}) {
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = std::move(code);
    diagnostic.category = "script.foundation";
    diagnostic.severity = severity;
    diagnostic.message = std::move(message);
    diagnostic.source = {location.file, location.line, location.column};
    if (!fix.empty()) {
        diagnostic.suggested_fixes.push_back(std::move(fix));
    }
    return diagnostic;
}

void EmitBlocking(Astra::Core::DiagnosticSink& diagnostics, const ScriptSourceLocation& location, std::string code, std::string message, std::string fix = {}) {
    diagnostics.Emit(MakeDiagnostic(std::move(code), Astra::Core::DiagnosticSeverity::Blocking, std::move(message), location, std::move(fix)));
}

bool SetAsset(ScriptCommand& command, std::string_view asset_text, Astra::Core::DiagnosticSink& diagnostics) {
    auto asset = Astra::Asset::ParseAssetUri(asset_text);
    if (!asset) {
        EmitBlocking(diagnostics, command.location, "ASTRA_SCRIPT_ASSET_INVALID", "Script command references an invalid asset URI.", "Use a stable URI such as native:/Backgrounds/Room.");
        return false;
    }
    command.asset = asset.Value();
    return true;
}

ScriptCommand MakeCommand(ScriptCommandKind kind, const ScriptSource& source, Astra::Core::u32 line, Astra::Core::u32 index) {
    ScriptCommand command;
    command.kind = kind;
    command.location = {source.file.empty() ? source.source_id : source.file, line, 1};
    command.command_id = source.source_id + "#" + std::to_string(index);
    return command;
}

void AddLabel(CompiledScript& script, const std::string& label, Astra::Core::u32 index, const ScriptSourceLocation& location, Astra::Core::DiagnosticSink& diagnostics) {
    if (label.empty()) {
        EmitBlocking(diagnostics, location, "ASTRA_SCRIPT_LABEL_MISSING", "Label command requires a name.", "Use: label opening");
        return;
    }
    if (script.labels.contains(label)) {
        EmitBlocking(diagnostics, location, "ASTRA_SCRIPT_LABEL_DUPLICATE", "Label is defined more than once.", "Rename one label or update jumps.");
        return;
    }
    script.labels[label] = index;
}

void AddDebugSymbol(CompiledScript& script, const ScriptCommand& command) {
    script.debug_symbols.push_back({command.command_id, command.location, command.label});
}

bool ValidateLabels(const CompiledScript& script, Astra::Core::DiagnosticSink& diagnostics) {
    bool valid = true;
    for (const auto& command : script.commands) {
        auto check_label = [&](const std::string& target) {
            if (!target.empty() && !script.labels.contains(target)) {
                EmitBlocking(diagnostics, command.location, "ASTRA_SCRIPT_LABEL_UNKNOWN", "Script references an unknown label.", "Add the label or fix the jump target.");
                valid = false;
            }
        };
        check_label(command.target_label);
        for (const auto& choice : command.choices) {
            check_label(choice.target_label);
        }
    }
    return valid;
}

void AppendCommand(CompiledScript& script, ScriptCommand command, Astra::Core::DiagnosticSink& diagnostics) {
    const auto index = static_cast<Astra::Core::u32>(script.commands.size());
    if (command.kind == ScriptCommandKind::Label) {
        AddLabel(script, command.label, index, command.location, diagnostics);
    }
    AddDebugSymbol(script, command);
    script.commands.push_back(std::move(command));
}

std::string ChoiceText(const std::vector<ScriptChoice>& choices) {
    std::string text;
    for (std::size_t index = 0; index < choices.size(); ++index) {
        if (index > 0) {
            text += " | ";
        }
        text += choices[index].text;
    }
    return text;
}

Astra::Core::EventTypeId EventType(std::string_view value) {
    return Astra::Core::StableId(Astra::Core::StableIdKind::EventType, std::string(value));
}

Astra::Runtime::RuntimeEvent MakeEvent(const ScriptCommand& command, std::string event_value, std::string category, std::string target, nlohmann::json payload) {
    Astra::Runtime::RuntimeEvent event;
    event.type = EventType(std::move(event_value));
    event.category = std::move(category);
    event.source = {"script", command.location.file};
    event.target = {"actor", std::move(target)};
    event.payload_schema = "astra.vn." + ToString(command.kind) + ".v1";
    event.payload = std::move(payload);
    event.trace.script_location = command.location.file + ":" + std::to_string(command.location.line);
    return event;
}

void AddPresentationForCommand(const ScriptCommand& command, std::vector<Astra::Media::PresentationCommand>& output, Astra::Core::u64 frame_index) {
    switch (command.kind) {
    case ScriptCommandKind::Background: {
        Astra::Media::PresentationCommand presentation;
        presentation.kind = Astra::Media::PresentationCommandKind::Sprite;
        presentation.command_id = command.command_id + ".background";
        presentation.frame_index = frame_index;
        presentation.layer = "background";
        presentation.order = 0;
        presentation.asset = command.asset;
        output.push_back(std::move(presentation));
        break;
    }
    case ScriptCommandKind::ShowCharacter: {
        Astra::Media::PresentationCommand presentation;
        presentation.kind = Astra::Media::PresentationCommandKind::Sprite;
        presentation.command_id = command.command_id + ".character";
        presentation.frame_index = frame_index;
        presentation.layer = "character";
        presentation.order = 100;
        presentation.asset = command.asset;
        presentation.payload = {{"actor", command.actor}, {"placement", command.placement}};
        output.push_back(std::move(presentation));
        break;
    }
    case ScriptCommandKind::Say: {
        Astra::Media::PresentationCommand presentation;
        presentation.kind = Astra::Media::PresentationCommandKind::Text;
        presentation.command_id = command.command_id + ".dialogue";
        presentation.frame_index = frame_index;
        presentation.layer = "text";
        presentation.order = 200;
        presentation.text = command.text;
        presentation.locale = "en-US";
        presentation.payload = {{"speaker", command.actor}, {"typewriter", {{"speed", 32}}}};
        output.push_back(std::move(presentation));
        if (!command.asset.path.empty()) {
            Astra::Media::PresentationCommand audio;
            audio.kind = Astra::Media::PresentationCommandKind::Audio;
            audio.command_id = command.command_id + ".voice";
            audio.frame_index = frame_index;
            audio.asset = command.asset;
            audio.bus = "voice";
            audio.payload = {{"kind", "play"}, {"volume", 1.0}, {"loop", false}};
            output.push_back(std::move(audio));
        }
        break;
    }
    case ScriptCommandKind::Choice: {
        Astra::Media::PresentationCommand presentation;
        presentation.kind = Astra::Media::PresentationCommandKind::Text;
        presentation.command_id = command.command_id + ".choice";
        presentation.frame_index = frame_index;
        presentation.layer = "ui";
        presentation.order = 210;
        presentation.text = ChoiceText(command.choices);
        presentation.payload = {{"choices", ToJson(command)["choices"]}};
        output.push_back(std::move(presentation));
        break;
    }
    case ScriptCommandKind::Audio: {
        Astra::Media::PresentationCommand presentation;
        presentation.kind = Astra::Media::PresentationCommandKind::Audio;
        presentation.command_id = command.command_id + ".audio";
        presentation.frame_index = frame_index;
        presentation.asset = command.asset;
        presentation.bus = command.payload.value("bus", "music");
        presentation.payload = {{"kind", "play"}, {"volume", 1.0}, {"loop", command.payload.value("loop", false)}};
        output.push_back(std::move(presentation));
        break;
    }
    case ScriptCommandKind::Filter: {
        Astra::Media::PresentationCommand presentation;
        presentation.kind = Astra::Media::PresentationCommandKind::Filter;
        presentation.command_id = command.command_id + ".filter";
        presentation.frame_index = frame_index;
        presentation.asset = command.asset;
        presentation.layer = "final";
        presentation.payload = {{"profile", command.asset.ToString()}};
        output.push_back(std::move(presentation));
        break;
    }
    default:
        break;
    }
}

} // namespace

std::vector<ScriptProviderDescriptor> FoundationScriptProviders() {
    return {
        {
            NativeRuntimeId,
            "Astra Native Script Foundation",
            false,
            true,
            true,
            true,
            {".astra"},
            {"compile_ir", "runtime_events", "presentation_commands", "snapshot"},
            {"project_read"},
        },
        {
            LuaRuntimeId,
            "Astra Lua Foundation",
            false,
            true,
            true,
            true,
            {".lua"},
            {"sol2_binding", "runtime_events", "presentation_commands", "snapshot"},
            {"project_read"},
        },
    };
}

ScriptRuntimeHost::ScriptRuntimeHost() : providers_(FoundationScriptProviders()) {}

const std::vector<ScriptProviderDescriptor>& ScriptRuntimeHost::Providers() const {
    return providers_;
}

Astra::Core::Result<CompiledScript> ScriptRuntimeHost::CompileNative(const ScriptSource& source, Astra::Core::DiagnosticSink& diagnostics) const {
    CompiledScript script;
    script.runtime_id = NativeRuntimeId;
    script.source_id = source.source_id;

    std::istringstream input(source.text);
    std::string line;
    Astra::Core::u32 line_number = 0;
    while (std::getline(input, line)) {
        ++line_number;
        auto trimmed = Trim(line);
        if (trimmed.empty() || StartsWith(trimmed, "#")) {
            continue;
        }
        const auto words = SplitWords(trimmed);
        if (words.empty()) {
            continue;
        }
        auto command = MakeCommand(ScriptCommandKind::Label, source, line_number, static_cast<Astra::Core::u32>(script.commands.size()));
        if (words[0] == "label" && words.size() >= 2) {
            command.kind = ScriptCommandKind::Label;
            command.label = words[1];
            AppendCommand(script, std::move(command), diagnostics);
        } else if (words[0] == "bg" && words.size() >= 2) {
            command.kind = ScriptCommandKind::Background;
            if (SetAsset(command, words[1], diagnostics)) {
                AppendCommand(script, std::move(command), diagnostics);
            }
        } else if (words[0] == "show" && words.size() >= 3) {
            command.kind = ScriptCommandKind::ShowCharacter;
            command.actor = words[1];
            command.placement = words.size() >= 4 ? words[3] : "center";
            if (SetAsset(command, words[2], diagnostics)) {
                AppendCommand(script, std::move(command), diagnostics);
            }
        } else if (words[0] == "say" && words.size() >= 3) {
            command.kind = ScriptCommandKind::Say;
            command.actor = words[1];
            auto text = ParseQuotedText(trimmed);
            if (!text) {
                EmitBlocking(diagnostics, command.location, "ASTRA_SCRIPT_SAY_TEXT_INVALID", "say requires quoted dialogue text.", "Use: say alice \"Hello.\"");
                continue;
            }
            command.text = *text;
            const auto voice_position = trimmed.find("voice ");
            if (voice_position != std::string::npos) {
                const auto voice_asset = Trim(std::string_view(trimmed).substr(voice_position + 6));
                (void)SetAsset(command, voice_asset, diagnostics);
            }
            AppendCommand(script, std::move(command), diagnostics);
        } else if (words[0] == "choice") {
            command.kind = ScriptCommandKind::Choice;
            if (auto route = ParseQuotedRoute(trimmed)) {
                command.choices.push_back({route->first, route->second});
            } else if (trimmed == "choice:") {
                while (std::getline(input, line)) {
                    ++line_number;
                    auto choice_line = Trim(line);
                    if (choice_line.empty() || StartsWith(choice_line, "#")) {
                        continue;
                    }
                    auto route = ParseQuotedRoute(choice_line);
                    if (!route) {
                        EmitBlocking(diagnostics, {source.file.empty() ? source.source_id : source.file, line_number, 1}, "ASTRA_SCRIPT_CHOICE_INVALID", "choice entry requires quoted text and a target label.", "Use: \"Choice\" -> target_label");
                        continue;
                    }
                    command.choices.push_back({route->first, route->second});
                    if (command.choices.size() >= 2) {
                        break;
                    }
                }
            }
            if (command.choices.empty()) {
                EmitBlocking(diagnostics, command.location, "ASTRA_SCRIPT_CHOICE_EMPTY", "choice requires at least one route.", "Use: choice \"Text\" -> target_label");
                continue;
            }
            AppendCommand(script, std::move(command), diagnostics);
        } else if (words[0] == "jump" && words.size() >= 2) {
            command.kind = ScriptCommandKind::Jump;
            command.target_label = words[1];
            AppendCommand(script, std::move(command), diagnostics);
        } else if (words[0] == "set" && words.size() >= 3) {
            command.kind = ScriptCommandKind::SetVariable;
            command.variable_name = words[1];
            command.value = words[2];
            AppendCommand(script, std::move(command), diagnostics);
        } else if (words[0] == "get" && words.size() >= 2) {
            command.kind = ScriptCommandKind::GetVariable;
            command.variable_name = words[1];
            AppendCommand(script, std::move(command), diagnostics);
        } else if (words[0] == "audio" && words.size() >= 3) {
            command.kind = ScriptCommandKind::Audio;
            command.payload = {{"bus", words[1]}, {"loop", words.size() >= 4 && words[3] == "loop"}};
            if (SetAsset(command, words[2], diagnostics)) {
                AppendCommand(script, std::move(command), diagnostics);
            }
        } else if (words[0] == "filter" && words.size() >= 2) {
            command.kind = ScriptCommandKind::Filter;
            if (SetAsset(command, words[1], diagnostics)) {
                AppendCommand(script, std::move(command), diagnostics);
            }
        } else {
            EmitBlocking(diagnostics, command.location, "ASTRA_SCRIPT_COMMAND_UNKNOWN", "Unknown Native DSL command.", "Use label, bg, show, say, choice, jump, set, get, audio, or filter.");
        }
    }

    if (!ValidateLabels(script, diagnostics) || diagnostics.HasBlocking()) {
        return Astra::Core::Result<CompiledScript>::Failure(Astra::Core::ErrorCode::InvalidFormat, "native script compilation failed");
    }
    return Astra::Core::Result<CompiledScript>::Success(std::move(script));
}

Astra::Core::Result<CompiledScript> ScriptRuntimeHost::CompileLua(const ScriptSource& source, Astra::Core::DiagnosticSink& diagnostics) const {
    CompiledScript script;
    script.runtime_id = LuaRuntimeId;
    script.source_id = source.source_id;
    try {
        sol::state lua;
        std::vector<ScriptCommand> commands;
        auto append = [&](ScriptCommand command) {
            command.command_id = source.source_id + "#" + std::to_string(commands.size());
            command.location = {source.file.empty() ? source.source_id : source.file, static_cast<Astra::Core::u32>(commands.size() + 1), 1};
            commands.push_back(std::move(command));
        };

        sol::table astra = lua.create_table();
        astra.set_function("label", [&](const std::string& label) {
            ScriptCommand command;
            command.kind = ScriptCommandKind::Label;
            command.label = label;
            append(std::move(command));
        });
        astra.set_function("bg", [&](const std::string& asset) {
            ScriptCommand command;
            command.kind = ScriptCommandKind::Background;
            auto parsed = Astra::Asset::ParseAssetUri(asset);
            if (parsed) {
                command.asset = parsed.Value();
            }
            append(std::move(command));
        });
        astra.set_function("show", [&](const std::string& actor, const std::string& asset, const std::string& placement) {
            ScriptCommand command;
            command.kind = ScriptCommandKind::ShowCharacter;
            command.actor = actor;
            command.placement = placement;
            auto parsed = Astra::Asset::ParseAssetUri(asset);
            if (parsed) {
                command.asset = parsed.Value();
            }
            append(std::move(command));
        });
        astra.set_function("say", [&](const std::string& actor, const std::string& text, const sol::optional<std::string>& voice) {
            ScriptCommand command;
            command.kind = ScriptCommandKind::Say;
            command.actor = actor;
            command.text = text;
            if (voice.has_value()) {
                auto parsed = Astra::Asset::ParseAssetUri(voice.value());
                if (parsed) {
                    command.asset = parsed.Value();
                }
            }
            append(std::move(command));
        });
        astra.set_function("choice", [&](const std::string& text, const std::string& target_label) {
            ScriptCommand command;
            command.kind = ScriptCommandKind::Choice;
            command.choices.push_back({text, target_label});
            append(std::move(command));
        });
        astra.set_function("jump", [&](const std::string& target_label) {
            ScriptCommand command;
            command.kind = ScriptCommandKind::Jump;
            command.target_label = target_label;
            append(std::move(command));
        });
        astra.set_function("set", [&](const std::string& name, const std::string& value) {
            ScriptCommand command;
            command.kind = ScriptCommandKind::SetVariable;
            command.variable_name = name;
            command.value = value;
            append(std::move(command));
        });
        astra.set_function("audio", [&](const std::string& bus, const std::string& asset) {
            ScriptCommand command;
            command.kind = ScriptCommandKind::Audio;
            command.payload = {{"bus", bus}, {"loop", false}};
            auto parsed = Astra::Asset::ParseAssetUri(asset);
            if (parsed) {
                command.asset = parsed.Value();
            }
            append(std::move(command));
        });
        astra.set_function("filter", [&](const std::string& asset) {
            ScriptCommand command;
            command.kind = ScriptCommandKind::Filter;
            auto parsed = Astra::Asset::ParseAssetUri(asset);
            if (parsed) {
                command.asset = parsed.Value();
            }
            append(std::move(command));
        });
        lua["astra"] = astra;
        lua.script(source.text);

        for (auto& command : commands) {
            if ((command.kind == ScriptCommandKind::Background || command.kind == ScriptCommandKind::ShowCharacter || command.kind == ScriptCommandKind::Audio || command.kind == ScriptCommandKind::Filter) && command.asset.path.empty()) {
                EmitBlocking(diagnostics, command.location, "ASTRA_SCRIPT_ASSET_INVALID", "Lua script command references an invalid asset URI.", "Use native:/ asset URIs.");
                continue;
            }
            AppendCommand(script, std::move(command), diagnostics);
        }
    } catch (const sol::error& error) {
        ScriptSourceLocation location{source.file.empty() ? source.source_id : source.file, 1, 1};
        EmitBlocking(diagnostics, location, "ASTRA_SCRIPT_LUA_ERROR", error.what(), "Use only the astra host API table in Phase 4 foundation Lua.");
    }

    if (!ValidateLabels(script, diagnostics) || diagnostics.HasBlocking()) {
        return Astra::Core::Result<CompiledScript>::Failure(Astra::Core::ErrorCode::InvalidFormat, "lua script compilation failed");
    }
    return Astra::Core::Result<CompiledScript>::Success(std::move(script));
}

ScriptExecutionResult ScriptRuntimeHost::Run(
    const CompiledScript& script,
    Astra::Runtime::RuntimeWorld& runtime,
    const ScriptExecutionOptions& options,
    Astra::Core::DiagnosticSink& diagnostics) const {
    return bridge_.Execute(script, runtime, options, diagnostics);
}

ScriptExecutionResult ScriptEventBridge::Execute(
    const CompiledScript& script,
    Astra::Runtime::RuntimeWorld& runtime,
    const ScriptExecutionOptions& options,
    Astra::Core::DiagnosticSink& diagnostics) const {
    ScriptExecutionResult result;
    result.snapshot.runtime_id = script.runtime_id;
    result.snapshot.source_id = script.source_id;
    auto start = script.labels.find(options.entry_label);
    std::size_t index = start == script.labels.end() ? 0 : start->second;
    std::unordered_set<std::size_t> visited_jumps;
    Astra::Core::u64 frame = 1;

    while (index < script.commands.size()) {
        const auto& command = script.commands[index];
        result.snapshot.current_command_index = static_cast<Astra::Core::u32>(index);
        result.snapshot.commands_executed.push_back(command.command_id);
        if (!command.label.empty()) {
            result.snapshot.active_label = command.label;
        }

        std::optional<Astra::Runtime::RuntimeEvent> event;
        switch (command.kind) {
        case ScriptCommandKind::Label:
            break;
        case ScriptCommandKind::Background:
            event = MakeEvent(command, "astra.vn.background.show", "vn.background", "actor:/systems/scene", {{"asset", command.asset.ToString()}});
            break;
        case ScriptCommandKind::ShowCharacter:
            event = MakeEvent(command, "astra.vn.character.show", "vn.character", "actor:/characters/" + command.actor, {{"actor", command.actor}, {"asset", command.asset.ToString()}, {"placement", command.placement}});
            break;
        case ScriptCommandKind::Say:
            event = MakeEvent(command, "astra.vn.dialogue.say_requested", "vn.dialogue", "actor:/systems/dialogue", {{"speaker", command.actor}, {"text", command.text}, {"voice", command.asset.ToString()}});
            break;
        case ScriptCommandKind::Choice: {
            nlohmann::json choices = nlohmann::json::array();
            for (const auto& choice : command.choices) {
                choices.push_back(ToJson(choice));
            }
            event = MakeEvent(command, "astra.vn.choice.presented", "vn.choice", "actor:/systems/choice", {{"choices", choices}});
            break;
        }
        case ScriptCommandKind::Jump:
            break;
        case ScriptCommandKind::SetVariable:
            result.snapshot.variables[command.variable_name] = command.value;
            event = MakeEvent(command, "astra.vn.variable.set", "vn.script", "actor:/systems/story_director", {{"name", command.variable_name}, {"value", command.value}});
            break;
        case ScriptCommandKind::GetVariable:
            event = MakeEvent(command, "astra.vn.variable.get", "vn.script", "actor:/systems/story_director", {{"name", command.variable_name}, {"value", result.snapshot.variables.value(command.variable_name, nlohmann::json())}});
            break;
        case ScriptCommandKind::Audio:
            event = MakeEvent(command, "astra.vn.audio.play", "vn.audio", "actor:/systems/audio", {{"asset", command.asset.ToString()}, {"bus", command.payload.value("bus", "music")}});
            break;
        case ScriptCommandKind::Filter:
            event = MakeEvent(command, "astra.vn.filter.apply", "vn.filter", "actor:/systems/filter", {{"profile", command.asset.ToString()}});
            break;
        }

        if (event.has_value()) {
            auto emitted = runtime.Emit(event.value(), Astra::Runtime::RuntimeEventMode::Queued, diagnostics);
            if (emitted) {
                result.events.push_back(event.value());
            }
        }
        AddPresentationForCommand(command, result.presentation_commands, frame++);

        if (command.kind == ScriptCommandKind::Choice && !command.choices.empty()) {
            const auto selected = std::min<std::size_t>(options.choice_index, command.choices.size() - 1);
            result.snapshot.last_choice = command.choices[selected].text;
            const auto label = script.labels.find(command.choices[selected].target_label);
            if (label != script.labels.end()) {
                index = label->second;
                continue;
            }
        }
        if (command.kind == ScriptCommandKind::Jump) {
            const auto label = script.labels.find(command.target_label);
            if (label != script.labels.end()) {
                if (visited_jumps.contains(index)) {
                    EmitBlocking(diagnostics, command.location, "ASTRA_SCRIPT_LOOP_DETECTED", "Foundation script execution detected a repeated jump.", "Use finite Phase 4 smoke scripts.");
                    break;
                }
                visited_jumps.insert(index);
                index = label->second;
                continue;
            }
        }
        ++index;
    }

    (void)runtime.Tick(diagnostics);
    return result;
}

std::string ToString(ScriptCommandKind kind) {
    switch (kind) {
    case ScriptCommandKind::Label:
        return "label";
    case ScriptCommandKind::Background:
        return "background";
    case ScriptCommandKind::ShowCharacter:
        return "show_character";
    case ScriptCommandKind::Say:
        return "say";
    case ScriptCommandKind::Choice:
        return "choice";
    case ScriptCommandKind::Jump:
        return "jump";
    case ScriptCommandKind::SetVariable:
        return "set_variable";
    case ScriptCommandKind::GetVariable:
        return "get_variable";
    case ScriptCommandKind::Audio:
        return "audio";
    case ScriptCommandKind::Filter:
        return "filter";
    }
    return "unknown";
}

nlohmann::json ToJson(const ScriptSourceLocation& location) {
    return {{"file", location.file}, {"line", location.line}, {"column", location.column}};
}

nlohmann::json ToJson(const ScriptChoice& choice) {
    return {{"text", choice.text}, {"target_label", choice.target_label}};
}

nlohmann::json ToJson(const ScriptCommand& command) {
    nlohmann::json choices = nlohmann::json::array();
    for (const auto& choice : command.choices) {
        choices.push_back(ToJson(choice));
    }
    return {
        {"kind", ToString(command.kind)},
        {"command_id", command.command_id},
        {"location", ToJson(command.location)},
        {"label", command.label},
        {"actor", command.actor},
        {"asset", command.asset.ToString()},
        {"placement", command.placement},
        {"text", command.text},
        {"target_label", command.target_label},
        {"variable_name", command.variable_name},
        {"value", command.value},
        {"choices", choices},
        {"payload", command.payload},
    };
}

nlohmann::json ToJson(const ScriptDebugSymbol& symbol) {
    return {{"command_id", symbol.command_id}, {"location", ToJson(symbol.location)}, {"label", symbol.label}};
}

nlohmann::json ToJson(const CompiledScript& script) {
    nlohmann::json commands = nlohmann::json::array();
    for (const auto& command : script.commands) {
        commands.push_back(ToJson(command));
    }
    nlohmann::json symbols = nlohmann::json::array();
    for (const auto& symbol : script.debug_symbols) {
        symbols.push_back(ToJson(symbol));
    }
    return {{"runtime_id", script.runtime_id}, {"source_id", script.source_id}, {"commands", commands}, {"labels", script.labels}, {"debug_symbols", symbols}};
}

nlohmann::json ToJson(const ScriptSnapshot& snapshot) {
    return {
        {"schema", snapshot.schema},
        {"runtime_id", snapshot.runtime_id},
        {"source_id", snapshot.source_id},
        {"current_command_index", snapshot.current_command_index},
        {"active_label", snapshot.active_label},
        {"variables", snapshot.variables},
        {"last_choice", snapshot.last_choice},
        {"commands_executed", snapshot.commands_executed},
    };
}

nlohmann::json ToJson(const ScriptProviderDescriptor& descriptor) {
    return {
        {"runtime_id", descriptor.runtime_id},
        {"display_name", descriptor.display_name},
        {"debug_supported", descriptor.debug_supported},
        {"snapshot_supported", descriptor.snapshot_supported},
        {"deterministic", descriptor.deterministic},
        {"packaged_eligible", descriptor.packaged_eligible},
        {"source_extensions", descriptor.source_extensions},
        {"capabilities", descriptor.capabilities},
        {"permissions", descriptor.permissions},
    };
}

nlohmann::json ToJson(const ScriptExecutionResult& result) {
    nlohmann::json events = nlohmann::json::array();
    for (const auto& event : result.events) {
        events.push_back(Astra::Runtime::ToJson(event));
    }
    nlohmann::json commands = nlohmann::json::array();
    for (const auto& command : result.presentation_commands) {
        commands.push_back(Astra::Media::ToJson(command));
    }
    return {{"events", events}, {"presentation_commands", commands}, {"snapshot", ToJson(result.snapshot)}};
}

} // namespace Astra::Script
