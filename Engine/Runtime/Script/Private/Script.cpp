#include "ScriptPrivate.hpp"

#include <Astra/Core/Logging.hpp>

#include <algorithm>
#include <cctype>
#include <sstream>

namespace Astra::Script {

namespace Private {

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

Astra::Core::Diagnostic MakeDiagnostic(
    std::string code,
    Astra::Core::DiagnosticSeverity severity,
    std::string message,
    const ScriptSourceLocation& location,
    std::string fix) {
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = std::move(code);
    diagnostic.category = "script.phase8";
    diagnostic.severity = severity;
    diagnostic.message = std::move(message);
    diagnostic.source = {location.file, location.line, location.column};
    if (!fix.empty()) {
        diagnostic.suggested_fixes.push_back(std::move(fix));
    }
    return diagnostic;
}

void EmitBlocking(Astra::Core::DiagnosticSink& diagnostics, const ScriptSourceLocation& location, std::string code, std::string message, std::string fix) {
    diagnostics.Emit(MakeDiagnostic(std::move(code), Astra::Core::DiagnosticSeverity::Blocking, std::move(message), location, std::move(fix)));
}

ScriptCommand MakeCommand(ScriptCommandKind kind, const ScriptSource& source, Astra::Core::u32 line, Astra::Core::u32 index) {
    ScriptCommand command;
    command.kind = kind;
    command.location = {source.file.empty() ? source.source_id : source.file, line, 1};
    command.command_id = source.source_id + "#cmd_" + std::to_string(index);
    return command;
}

void AppendCommand(CompiledScript& script, ScriptCommand command, Astra::Core::DiagnosticSink& diagnostics) {
    const auto index = static_cast<Astra::Core::u32>(script.commands.size());
    if (command.command_id.empty()) {
        command.command_id = script.source_id + "#cmd_" + std::to_string(index);
    }
    BindCommandSchema(command, diagnostics);
    if (command.kind == ScriptCommandKind::Label || command.kind == ScriptCommandKind::Scene) {
        const auto label = command.kind == ScriptCommandKind::Scene ? command.scene : command.label;
        if (!label.empty()) {
            if (script.labels.contains(label)) {
                EmitBlocking(diagnostics, command.location, "ASTRA_SCRIPT_LABEL_DUPLICATE", "Script label or scene is defined more than once.", "Rename the scene or label.");
            } else {
                script.labels[label] = index;
            }
        }
    }
    script.debug_symbols.push_back({command.command_id, command.stable_id, command.location, command.label.empty() ? command.scene : command.label, ToString(command.kind)});
    script.source_map.entries.push_back({command.command_id, command.stable_id, command.location, command.stable_id, command.kind == ScriptCommandKind::Timeline ? command.stable_id : ""});
    script.document.nodes.push_back({command.stable_id, ToString(command.kind), command.location, ToJson(command)});
    if (!command.schema_id.empty()) {
        script.command_manifest.push_back({{"schema_id", command.schema_id}, {"command_id", command.command_id}, {"stable_id", command.stable_id}, {"source", ToJson(command.location)}});
    }
    AddIrForCommand(script, command);
    script.commands.push_back(std::move(command));
}

bool ValidateCompiledScript(const CompiledScript& script, Astra::Core::DiagnosticSink& diagnostics) {
    bool valid = true;
    for (const auto& command : script.commands) {
        auto check_target = [&](const std::string& target) {
            if (!target.empty() && !script.labels.contains(target)) {
                EmitBlocking(diagnostics, command.location, "ASTRA_SCRIPT_TARGET_UNKNOWN", "Script references an unknown scene or label.", "Add the target scene or fix the transition.");
                valid = false;
            }
        };
        check_target(command.target_label);
        for (const auto& choice : command.choices) {
            check_target(choice.target_label);
        }
    }
    return valid && !diagnostics.HasBlocking();
}

Astra::Core::EventTypeId EventType(std::string_view value) {
    return Astra::Core::StableId(Astra::Core::StableIdKind::EventType, std::string(value));
}

Astra::Runtime::RuntimeEvent MakeEvent(const ScriptCommand& command, std::string event_value, std::string category, std::string target, nlohmann::json payload) {
    Astra::Runtime::RuntimeEvent event;
    event.type = EventType(std::move(event_value));
    event.category = std::move(category);
    event.source = {"script", command.command_id.empty() ? command.location.file : command.command_id};
    event.target = target == "broadcast" ? Astra::Runtime::RuntimeEventEndpoint{"broadcast", ""} : Astra::Runtime::RuntimeEventEndpoint{"actor", std::move(target)};
    event.payload_schema = "astra.vn." + ToString(command.kind) + ".v1";
    event.payload = std::move(payload);
    event.trace.script_location = command.location.file + ":" + std::to_string(command.location.line);
    return event;
}

std::string SchemaIdForCommand(const ScriptCommand& command) {
    if (!command.extension_id.empty() && !command.extension_command.empty()) {
        return command.extension_id + "." + command.extension_command;
    }
    switch (command.kind) {
    case ScriptCommandKind::Scene: return "astra.vn.scene.enter";
    case ScriptCommandKind::Stage: return "astra.vn.stage.snapshot";
    case ScriptCommandKind::Background: return "astra.vn.background.show";
    case ScriptCommandKind::ShowCharacter: return "astra.vn.character.show";
    case ScriptCommandKind::HideCharacter: return "astra.vn.character.hide";
    case ScriptCommandKind::Pose:
    case ScriptCommandKind::Move: return "astra.vn.character.update";
    case ScriptCommandKind::Camera: return "astra.vn.camera.update";
    case ScriptCommandKind::Timeline: return "astra.vn.timeline.start";
    case ScriptCommandKind::Say: return "astra.vn.dialogue.say";
    case ScriptCommandKind::Choice: return "astra.vn.choice.present";
    case ScriptCommandKind::SetVariable:
    case ScriptCommandKind::LetVariable:
    case ScriptCommandKind::IncrementVariable:
    case ScriptCommandKind::DecrementVariable:
    case ScriptCommandKind::ToggleVariable:
    case ScriptCommandKind::PushVariable:
    case ScriptCommandKind::RemoveVariable:
    case ScriptCommandKind::ClearVariable: return "astra.vn.variable.write";
    case ScriptCommandKind::Audio: return "astra.vn.audio.play";
    case ScriptCommandKind::Filter: return "astra.vn.filter.apply";
    case ScriptCommandKind::Await: return "astra.vn.script.await";
    case ScriptCommandKind::Jump: return "astra.vn.flow.jump";
    case ScriptCommandKind::End: return "astra.vn.flow.end";
    case ScriptCommandKind::Return: return "astra.vn.flow.return";
    default: return {};
    }
}

const ScriptCommandSchema* FindCommandSchema(std::string_view schema_id) {
    static const auto schemas = FoundationCommandSchemas();
    const auto found = std::ranges::find_if(schemas, [&](const ScriptCommandSchema& schema) {
        return schema.schema_id == schema_id;
    });
    return found == schemas.end() ? nullptr : &*found;
}

void BindCommandSchema(ScriptCommand& command, Astra::Core::DiagnosticSink& diagnostics) {
    command.schema_id = SchemaIdForCommand(command);
    if (command.schema_id.empty()) {
        return;
    }
    const auto* schema = FindCommandSchema(command.schema_id);
    if (schema == nullptr) {
        EmitBlocking(diagnostics, command.location, "ASTRA_SCRIPT_COMMAND_SCHEMA_UNKNOWN", "Script command has no registered command schema.", "Register an AstraVN built-in schema or Lua extension schema fixture before cook/package.");
        return;
    }
    command.payload["schema_id"] = schema->schema_id;
    command.payload["policy"] = schema->policy;
    if (!schema->channels.empty()) {
        command.payload["channels"] = schema->channels;
    }
}

void AddPresentationForCommand(const ScriptCommand& command, std::vector<Astra::Media::PresentationCommand>& output, Astra::Core::u64 frame_index) {
    Astra::Media::PresentationCommand presentation;
    presentation.command_id = command.command_id;
    presentation.frame_index = frame_index;
    presentation.payload = command.payload;
    switch (command.kind) {
    case ScriptCommandKind::Background:
        presentation.kind = Astra::Media::PresentationCommandKind::Sprite;
        presentation.layer = "background";
        presentation.order = 0;
        presentation.asset = command.asset;
        break;
    case ScriptCommandKind::ShowCharacter:
    case ScriptCommandKind::Pose:
    case ScriptCommandKind::Move:
        presentation.kind = Astra::Media::PresentationCommandKind::Sprite;
        presentation.layer = "character";
        presentation.order = 100;
        presentation.asset = command.asset;
        presentation.payload.update({{"actor", command.actor}, {"placement", command.placement}});
        break;
    case ScriptCommandKind::Say:
        presentation.kind = Astra::Media::PresentationCommandKind::Text;
        presentation.layer = "text";
        presentation.order = 200;
        presentation.text = command.text;
        presentation.locale = "en-US";
        presentation.payload.update({{"speaker", command.actor}, {"stable_id", command.stable_id}});
        break;
    case ScriptCommandKind::Choice:
        presentation.kind = Astra::Media::PresentationCommandKind::Text;
        presentation.layer = "ui";
        presentation.order = 210;
        presentation.text = command.text;
        presentation.payload.update({{"choices", ToJson(command)["choices"]}});
        break;
    case ScriptCommandKind::Audio:
        presentation.kind = Astra::Media::PresentationCommandKind::Audio;
        presentation.asset = command.asset;
        presentation.bus = command.payload.value("bus", "music");
        presentation.payload.update({{"kind", "play"}});
        break;
    case ScriptCommandKind::Camera:
        presentation.kind = Astra::Media::PresentationCommandKind::Timeline;
        presentation.layer = "final";
        presentation.order = 50;
        presentation.payload.update({{"track", "camera"}});
        break;
    case ScriptCommandKind::Timeline:
        presentation.kind = Astra::Media::PresentationCommandKind::Timeline;
        presentation.layer = "final";
        presentation.order = 40;
        break;
    case ScriptCommandKind::Filter:
        presentation.kind = Astra::Media::PresentationCommandKind::Filter;
        presentation.layer = "final";
        presentation.asset = command.asset;
        break;
    default:
        return;
    }
    output.push_back(std::move(presentation));
}

void AddIrForCommand(CompiledScript& script, const ScriptCommand& command) {
    if (command.kind == ScriptCommandKind::State) {
        script.state_graph.states.push_back({{"id", command.stable_id}, {"name", command.state}});
    } else if (command.kind == ScriptCommandKind::Scene) {
        script.state_graph.scenes.push_back({{"id", command.stable_id}, {"name", command.scene}, {"state", command.state}});
    } else if (command.kind == ScriptCommandKind::Jump || command.kind == ScriptCommandKind::Await || command.kind == ScriptCommandKind::End) {
        script.state_graph.transitions.push_back({{"command", command.command_id}, {"target", command.target_label}, {"kind", ToString(command.kind)}});
    } else if (command.kind == ScriptCommandKind::Timeline || command.kind == ScriptCommandKind::Camera || command.kind == ScriptCommandKind::Extension) {
        script.effects.effects.push_back(ToJson(command));
        if (command.payload.contains("channels")) {
            for (const auto& channel : command.payload["channels"]) {
                script.effects.channels.push_back(channel);
            }
        }
    } else {
        script.narrative.commands.push_back(ToJson(command));
    }
}

} // namespace Private

std::vector<ScriptProviderDescriptor> FoundationScriptProviders() {
    return {
        {
            NativeRuntimeId,
            "Astra Native Script Phase 8",
            true,
            true,
            true,
            true,
            {".astra"},
            {"ast", "state_graph_ir", "narrative_ir", "effect_graph_ir", "source_map", "debug", "hot_reload", "snapshot_v2"},
            {"project_read"},
        },
        {
            LuaExtensionRuntimeId,
            "Astra Lua Extension Package SDK",
            true,
            true,
            true,
            true,
            {".lua"},
            {"extension_schema", "sandbox", "command_manifest"},
            {"project_read"},
        },
    };
}

std::vector<ScriptCommandSchema> FoundationCommandSchemas() {
    auto vn = [](std::string id, std::string event, std::string category, std::string target, std::vector<std::string> channels, nlohmann::json presentation = nlohmann::json::object()) {
        return ScriptCommandSchema{
            std::move(id),
            "astra.vn",
            "",
            1,
            nlohmann::json::object(),
            {{"event", std::move(event)}, {"category", std::move(category)}, {"target", std::move(target)}},
            std::move(presentation),
            nlohmann::json::object(),
            {{"save", "serializable"}, {"skip", "finish"}, {"rollback", "snapshot"}, {"debug", "source_mapped"}},
            std::move(channels),
            true,
            false,
        };
    };
    return {
        vn("astra.vn.scene.enter", "astra.vn.scene.enter", "vn.scene", "actor:/systems/story_director", {"story"}),
        vn("astra.vn.stage.snapshot", "astra.vn.stage.snapshot", "vn.stage", "actor:/systems/scene", {"stage"}),
        vn("astra.vn.background.show", "astra.vn.background.show", "vn.background", "actor:/systems/scene", {"background"}, {{"kind", "sprite"}, {"layer", "background"}}),
        vn("astra.vn.character.show", "astra.vn.character.show", "vn.character", "actor:/characters/{actor}", {"character"}, {{"kind", "sprite"}, {"layer", "character"}}),
        vn("astra.vn.character.hide", "astra.vn.character.hide", "vn.character", "actor:/characters/{actor}", {"character"}),
        vn("astra.vn.character.update", "astra.vn.character.update", "vn.character", "actor:/characters/{actor}", {"character"}, {{"kind", "sprite"}, {"layer", "character"}}),
        vn("astra.vn.camera.update", "astra.vn.camera.update", "vn.camera", "actor:/systems/camera", {"camera"}, {{"kind", "timeline"}, {"layer", "final"}}),
        vn("astra.vn.timeline.start", "astra.vn.timeline.start", "vn.timeline", "actor:/systems/story_director", {"timeline"}, {{"kind", "timeline"}, {"layer", "final"}}),
        vn("astra.vn.dialogue.say", "astra.vn.dialogue.say_requested", "vn.dialogue", "actor:/systems/dialogue", {"text"}, {{"kind", "text"}, {"layer", "text"}}),
        vn("astra.vn.choice.present", "astra.vn.choice.presented", "vn.choice", "broadcast", {"ui"}, {{"kind", "text"}, {"layer", "ui"}}),
        vn("astra.vn.variable.write", "astra.vn.variable.write", "vn.script", "actor:/systems/story_director", {"script"}),
        vn("astra.vn.audio.play", "astra.vn.audio.play", "vn.audio", "actor:/systems/audio", {"audio"}, {{"kind", "audio"}}),
        vn("astra.vn.filter.apply", "astra.vn.filter.apply", "vn.filter", "actor:/systems/filter", {"filter"}, {{"kind", "filter"}, {"layer", "final"}}),
        vn("astra.vn.script.await", "astra.vn.script.await", "vn.script", "actor:/systems/story_director", {"script"}),
        vn("astra.vn.flow.jump", "astra.vn.flow.jump", "vn.script", "actor:/systems/story_director", {"script"}),
        vn("astra.vn.flow.end", "astra.vn.flow.end", "vn.script", "actor:/systems/story_director", {"script"}),
        vn("astra.vn.flow.return", "astra.vn.flow.return", "vn.script", "actor:/systems/story_director", {"script"}),
        {"live2d.motion.play", "live2d", "motion.play", 1, {{"actor", {{"type", "ActorRef"}, {"required", true}}}, {"motion", {{"type", "string"}, {"required", true}}}}, {{"event", "astra.vn.extension.command"}, {"category", "vn.extension"}, {"target", "actor:/systems/story_director"}}, nlohmann::json::object(), nlohmann::json::object(), {{"save", "serializable"}, {"skip", "finish"}, {"rollback", "snapshot"}, {"debug", "source_mapped"}}, {"live2d.motion"}, true, true},
    };
}

ScriptRuntimeHost::ScriptRuntimeHost() : providers_(FoundationScriptProviders()), command_schemas_(FoundationCommandSchemas()) {}

const std::vector<ScriptProviderDescriptor>& ScriptRuntimeHost::Providers() const {
    return providers_;
}

const std::vector<ScriptCommandSchema>& ScriptRuntimeHost::CommandSchemas() const {
    return command_schemas_;
}

ScriptExecutionResult ScriptRuntimeHost::Run(
    const CompiledScript& script,
    Astra::Runtime::RuntimeWorld& runtime,
    const ScriptExecutionOptions& options,
    Astra::Core::DiagnosticSink& diagnostics) const {
    Astra::Core::DefaultLogger().Log(
        "script.execute",
        script.runtime_id,
        Astra::Core::LogLevel::Info,
        "script execution started",
        {{"source", script.source_id}, {"entry_label", options.entry_label}, {"commands", std::to_string(script.commands.size())}});
    auto result = bridge_.Execute(script, runtime, options, diagnostics);
    Astra::Core::DefaultLogger().Log(
        "script.execute",
        script.runtime_id,
        Astra::Core::LogLevel::Info,
        "script execution finished",
        {{"source", script.source_id}, {"events", std::to_string(result.events.size())}, {"presentation_commands", std::to_string(result.presentation_commands.size())}});
    return result;
}

ScriptStepResult ScriptRuntimeHost::Step(
    const CompiledScript& script,
    const ScriptSnapshot& snapshot,
    Astra::Runtime::RuntimeWorld& runtime,
    Astra::Core::DiagnosticSink& diagnostics) const {
    ScriptStepResult step;
    if (snapshot.current_command_index >= script.commands.size()) {
        step.completed = true;
        return step;
    }
    CompiledScript one = script;
    one.commands = {script.commands[snapshot.current_command_index]};
    auto result = bridge_.Execute(one, runtime, {"", 0}, diagnostics);
    step.paused = true;
    step.completed = snapshot.current_command_index + 1 >= script.commands.size();
    step.current_command = one.commands.front();
    step.location = step.current_command.location;
    step.variables = result.snapshot.variables;
    step.events = std::move(result.events);
    step.presentation_commands = std::move(result.presentation_commands);
    return step;
}

ScriptCompatibilityReport ScriptRuntimeHost::CheckHotReloadCompatibility(const CompiledScript& old_script, const CompiledScript& new_script, const ScriptSnapshot& snapshot) const {
    ScriptCompatibilityReport report;
    const auto active = snapshot.current_stable_id.empty() ? snapshot.current_command_id : snapshot.current_stable_id;
    for (const auto& command : old_script.commands) {
        const auto id = command.stable_id.empty() ? command.command_id : command.stable_id;
        if (id == active) {
            const auto compatible = std::ranges::find_if(new_script.commands, [&](const auto& candidate) {
                return (candidate.stable_id.empty() ? candidate.command_id : candidate.stable_id) == id && candidate.kind == command.kind;
            });
            report.compatible = compatible != new_script.commands.end();
            if (report.compatible) {
                report.migrated_command_ids.push_back(id);
            } else {
                report.incompatible_command_ids.push_back(id);
            }
            return report;
        }
    }
    report.compatible = active.empty();
    return report;
}

ScriptHotReloadReport ScriptRuntimeHost::HotReload(const CompiledScript& old_script, const CompiledScript& new_script, const ScriptSnapshot& snapshot) const {
    ScriptHotReloadReport report;
    report.compatibility = CheckHotReloadCompatibility(old_script, new_script, snapshot);
    report.reloaded = report.compatibility.compatible;
    report.rolled_back = !report.reloaded;
    return report;
}

} // namespace Astra::Script
