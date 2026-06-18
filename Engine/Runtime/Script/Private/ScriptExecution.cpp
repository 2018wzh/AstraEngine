#include "ScriptPrivate.hpp"

#include <algorithm>
#include <unordered_set>

namespace Astra::Script {

namespace {

std::optional<Astra::Runtime::RuntimeEvent> EventForCommand(const ScriptCommand& command) {
    switch (command.kind) {
    case ScriptCommandKind::Scene:
        return Private::MakeEvent(command, "astra.vn.scene.enter", "vn.scene", "actor:/systems/story_director", {{"scene", command.scene}, {"state", command.state}});
    case ScriptCommandKind::Stage:
        return Private::MakeEvent(command, "astra.vn.stage.snapshot", "vn.stage", "actor:/systems/scene", command.payload);
    case ScriptCommandKind::Background:
        return Private::MakeEvent(command, "astra.vn.background.show", "vn.background", "actor:/systems/scene", {{"asset", command.asset.ToString()}, {"stable_id", command.stable_id}});
    case ScriptCommandKind::ShowCharacter:
        return Private::MakeEvent(command, "astra.vn.character.show", "vn.character", "actor:/characters/" + command.actor, {{"actor", command.actor}, {"asset", command.asset.ToString()}, {"placement", command.placement}, {"stable_id", command.stable_id}});
    case ScriptCommandKind::Pose:
    case ScriptCommandKind::Move:
        return Private::MakeEvent(command, "astra.vn.character.update", "vn.character", "actor:/characters/" + command.actor, {{"actor", command.actor}, {"placement", command.placement}, {"payload", command.payload}, {"stable_id", command.stable_id}});
    case ScriptCommandKind::Camera:
        return Private::MakeEvent(command, "astra.vn.camera.update", "vn.camera", "actor:/systems/camera", {{"camera", command.payload}, {"stable_id", command.stable_id}});
    case ScriptCommandKind::Timeline:
        return Private::MakeEvent(command, "astra.vn.timeline.start", "vn.timeline", "actor:/systems/story_director", {{"timeline", command.payload}, {"stable_id", command.stable_id}});
    case ScriptCommandKind::Say:
        return Private::MakeEvent(command, "astra.vn.dialogue.say_requested", "vn.dialogue", "actor:/systems/dialogue", {{"speaker", command.actor}, {"text", command.text}, {"stable_id", command.stable_id}, {"voice", command.asset.ToString()}});
    case ScriptCommandKind::Choice: {
        nlohmann::json choices = nlohmann::json::array();
        for (const auto& choice : command.choices) {
            choices.push_back(ToJson(choice));
        }
        return Private::MakeEvent(command, "astra.vn.choice.presented", "vn.choice", "broadcast", {{"prompt", command.text}, {"choices", choices}, {"stable_id", command.stable_id}});
    }
    case ScriptCommandKind::SetVariable:
    case ScriptCommandKind::LetVariable:
    case ScriptCommandKind::IncrementVariable:
    case ScriptCommandKind::DecrementVariable:
    case ScriptCommandKind::ToggleVariable:
    case ScriptCommandKind::PushVariable:
    case ScriptCommandKind::RemoveVariable:
    case ScriptCommandKind::ClearVariable:
        return Private::MakeEvent(command, "astra.vn.variable.write", "vn.script", "actor:/systems/story_director", {{"name", command.variable_name}, {"value", command.value}, {"op", ToString(command.kind)}});
    case ScriptCommandKind::Audio:
        return Private::MakeEvent(command, "astra.vn.audio.play", "vn.audio", "actor:/systems/audio", {{"asset", command.asset.ToString()}, {"bus", command.payload.value("bus", "music")}, {"stable_id", command.stable_id}});
    case ScriptCommandKind::Filter:
        return Private::MakeEvent(command, "astra.vn.filter.apply", "vn.filter", "actor:/systems/filter", {{"profile", command.asset.ToString()}, {"stable_id", command.stable_id}});
    case ScriptCommandKind::Await:
        return Private::MakeEvent(command, "astra.vn.script.await", "vn.script", "actor:/systems/story_director", command.payload);
    case ScriptCommandKind::Extension:
        return Private::MakeEvent(command, "astra.vn.extension.command", "vn.extension", "actor:/systems/story_director", {{"extension", command.extension_id}, {"command", command.extension_command}, {"args", command.payload.value("args", nlohmann::json::object())}, {"stable_id", command.stable_id}});
    default:
        return std::nullopt;
    }
}

void ApplyVariable(ScriptSnapshot& snapshot, const ScriptCommand& command) {
    if (command.kind == ScriptCommandKind::SetVariable || command.kind == ScriptCommandKind::LetVariable) {
        snapshot.variables[command.variable_name] = command.value;
    } else if (command.kind == ScriptCommandKind::IncrementVariable) {
        snapshot.variables[command.variable_name] = snapshot.variables.value(command.variable_name, 0) + command.value.get<int>();
    } else if (command.kind == ScriptCommandKind::DecrementVariable) {
        snapshot.variables[command.variable_name] = snapshot.variables.value(command.variable_name, 0) - 1;
    } else if (command.kind == ScriptCommandKind::ToggleVariable) {
        snapshot.variables[command.variable_name] = !snapshot.variables.value(command.variable_name, false);
    }
}

void UpdateSnapshot(ScriptSnapshot& snapshot, const ScriptCommand& command, Astra::Core::u32 index) {
    snapshot.current_command_index = index;
    snapshot.current_command_id = command.command_id;
    snapshot.current_stable_id = command.stable_id;
    snapshot.active_story = command.story;
    snapshot.active_state = command.state;
    snapshot.active_scene = command.scene;
    if (!command.label.empty()) {
        snapshot.active_label = command.label;
    }
    if (command.kind == ScriptCommandKind::Timeline) {
        snapshot.active_timeline = command.stable_id;
        snapshot.timeline_state[command.stable_id] = command.payload;
    } else if (command.kind == ScriptCommandKind::Stage) {
        snapshot.stage_state["stage"] = command.payload;
    } else if (command.kind == ScriptCommandKind::Background) {
        snapshot.stage_state["background"] = command.asset.ToString();
    } else if (command.kind == ScriptCommandKind::ShowCharacter || command.kind == ScriptCommandKind::Pose || command.kind == ScriptCommandKind::Move) {
        snapshot.stage_state["characters"][command.actor] = {{"asset", command.asset.ToString()}, {"placement", command.placement}, {"payload", command.payload}};
    } else if (command.kind == ScriptCommandKind::Camera) {
        snapshot.stage_state["camera"] = command.payload;
    } else if (command.kind == ScriptCommandKind::Filter) {
        snapshot.stage_state["filter"] = command.asset.ToString();
    } else if (command.kind == ScriptCommandKind::Choice) {
        snapshot.choice_state["active_choice"] = command.stable_id;
    } else if (command.kind == ScriptCommandKind::Await) {
        snapshot.waits.push_back({{"command", command.command_id}, {"payload", command.payload}});
    }
    snapshot.commands_executed.push_back(command.command_id);
    ApplyVariable(snapshot, command);
}

} // namespace

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
        UpdateSnapshot(result.snapshot, command, static_cast<Astra::Core::u32>(index));

        if (auto event = EventForCommand(command)) {
            auto emitted = runtime.Emit(event.value(), Astra::Runtime::RuntimeEventMode::Queued, diagnostics);
            if (emitted) {
                result.events.push_back(event.value());
            }
            if (command.kind == ScriptCommandKind::Await) {
                Astra::Runtime::RuntimeTask task;
                task.owner = {"script", command.command_id};
                task.wait.kind = Astra::Runtime::RuntimeWaitKind::Script;
                task.wait.script_state = command.stable_id;
                task.continuation = {{"source", script.source_id}, {"command_index", index}, {"stable_id", command.stable_id}};
                (void)runtime.ScheduleTask(task, diagnostics);
            }
        }

        Private::AddPresentationForCommand(command, result.presentation_commands, frame++);

        if (command.kind == ScriptCommandKind::Choice && !command.choices.empty()) {
            const auto selected = std::min<std::size_t>(options.choice_index, command.choices.size() - 1);
            result.snapshot.last_choice = command.choices[selected].text;
            result.snapshot.choice_state["selected"] = ToJson(command.choices[selected]);
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
                    Private::EmitBlocking(diagnostics, command.location, "ASTRA_SCRIPT_LOOP_DETECTED", "Script execution detected a repeated transition.", "Use finite replay-safe story flow.");
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

} // namespace Astra::Script
