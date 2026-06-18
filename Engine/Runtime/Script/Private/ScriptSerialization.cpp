#include "ScriptPrivate.hpp"

namespace Astra::Script {

std::string ToString(ScriptCommandKind kind) {
    switch (kind) {
    case ScriptCommandKind::Label: return "label";
    case ScriptCommandKind::Story: return "story";
    case ScriptCommandKind::State: return "state";
    case ScriptCommandKind::Scene: return "scene";
    case ScriptCommandKind::Stage: return "stage";
    case ScriptCommandKind::Timeline: return "timeline";
    case ScriptCommandKind::Background: return "background";
    case ScriptCommandKind::ShowCharacter: return "show_character";
    case ScriptCommandKind::HideCharacter: return "hide_character";
    case ScriptCommandKind::Pose: return "pose";
    case ScriptCommandKind::Move: return "move";
    case ScriptCommandKind::Camera: return "camera";
    case ScriptCommandKind::Say: return "say";
    case ScriptCommandKind::Choice: return "choice";
    case ScriptCommandKind::Jump: return "jump";
    case ScriptCommandKind::Call: return "call";
    case ScriptCommandKind::Return: return "return";
    case ScriptCommandKind::Emit: return "emit";
    case ScriptCommandKind::Await: return "await";
    case ScriptCommandKind::Wait: return "wait";
    case ScriptCommandKind::End: return "end";
    case ScriptCommandKind::SetVariable: return "set_variable";
    case ScriptCommandKind::LetVariable: return "let_variable";
    case ScriptCommandKind::IncrementVariable: return "increment_variable";
    case ScriptCommandKind::DecrementVariable: return "decrement_variable";
    case ScriptCommandKind::ToggleVariable: return "toggle_variable";
    case ScriptCommandKind::PushVariable: return "push_variable";
    case ScriptCommandKind::RemoveVariable: return "remove_variable";
    case ScriptCommandKind::ClearVariable: return "clear_variable";
    case ScriptCommandKind::GetVariable: return "get_variable";
    case ScriptCommandKind::Audio: return "audio";
    case ScriptCommandKind::Filter: return "filter";
    case ScriptCommandKind::Sequence: return "sequence";
    case ScriptCommandKind::Parallel: return "parallel";
    case ScriptCommandKind::Race: return "race";
    case ScriptCommandKind::Spawn: return "spawn";
    case ScriptCommandKind::Cancel: return "cancel";
    case ScriptCommandKind::Extension: return "extension";
    }
    return "unknown";
}

nlohmann::json ToJson(const ScriptSourceLocation& location) {
    return {{"file", location.file}, {"line", location.line}, {"column", location.column}};
}

nlohmann::json ToJson(const ScriptChoice& choice) {
    return {{"stable_id", choice.stable_id}, {"text", choice.text}, {"target_label", choice.target_label}, {"guard", choice.guard}};
}

nlohmann::json ToJson(const ScriptCommand& command) {
    nlohmann::json choices = nlohmann::json::array();
    for (const auto& choice : command.choices) {
        choices.push_back(ToJson(choice));
    }
    return {
        {"kind", ToString(command.kind)},
        {"command_id", command.command_id},
        {"stable_id", command.stable_id},
        {"location", ToJson(command.location)},
        {"story", command.story},
        {"state", command.state},
        {"scene", command.scene},
        {"label", command.label},
        {"actor", command.actor},
        {"asset", command.asset.ToString()},
        {"placement", command.placement},
        {"text", command.text},
        {"target_label", command.target_label},
        {"schema_id", command.schema_id},
        {"extension_id", command.extension_id},
        {"extension_command", command.extension_command},
        {"variable_name", command.variable_name},
        {"value", command.value},
        {"choices", choices},
        {"payload", command.payload},
    };
}

nlohmann::json ToJson(const ScriptDebugSymbol& symbol) {
    return {{"command_id", symbol.command_id}, {"stable_id", symbol.stable_id}, {"location", ToJson(symbol.location)}, {"label", symbol.label}, {"node_kind", symbol.node_kind}};
}

nlohmann::json ToJson(const ScriptSourceMapEntry& entry) {
    return {{"command_id", entry.command_id}, {"stable_id", entry.stable_id}, {"location", ToJson(entry.location)}, {"graph_node", entry.graph_node}, {"timeline_key", entry.timeline_key}};
}

nlohmann::json ToJson(const ScriptDocument& document) {
    nlohmann::json nodes = nlohmann::json::array();
    for (const auto& node : document.nodes) {
        nodes.push_back({{"stable_id", node.stable_id}, {"kind", node.kind}, {"location", ToJson(node.location)}, {"fields", node.fields}});
    }
    return {{"schema", document.schema}, {"source_id", document.source_id}, {"nodes", nodes}, {"cst", document.cst}};
}

nlohmann::json ToJson(const StateGraphIr& ir) {
    return {{"schema", ir.schema}, {"states", ir.states}, {"scenes", ir.scenes}, {"transitions", ir.transitions}};
}

nlohmann::json ToJson(const NarrativeIr& ir) {
    return {{"schema", ir.schema}, {"commands", ir.commands}};
}

nlohmann::json ToJson(const EffectGraphIr& ir) {
    return {{"schema", ir.schema}, {"effects", ir.effects}, {"channels", ir.channels}};
}

nlohmann::json ToJson(const ScriptSourceMap& source_map) {
    nlohmann::json entries = nlohmann::json::array();
    for (const auto& entry : source_map.entries) {
        entries.push_back(ToJson(entry));
    }
    return {{"schema", source_map.schema}, {"entries", entries}};
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
    return {
        {"runtime_id", script.runtime_id},
        {"source_id", script.source_id},
        {"document", ToJson(script.document)},
        {"state_graph", ToJson(script.state_graph)},
        {"narrative", ToJson(script.narrative)},
        {"effects", ToJson(script.effects)},
        {"source_map", ToJson(script.source_map)},
        {"commands", commands},
        {"labels", script.labels},
        {"debug_symbols", symbols},
        {"command_manifest", script.command_manifest},
        {"extension_manifest", script.extension_manifest},
    };
}

nlohmann::json ToJson(const ScriptSnapshot& snapshot) {
    return {
        {"schema", snapshot.schema},
        {"runtime_id", snapshot.runtime_id},
        {"source_id", snapshot.source_id},
        {"current_command_index", snapshot.current_command_index},
        {"current_command_id", snapshot.current_command_id},
        {"current_stable_id", snapshot.current_stable_id},
        {"active_story", snapshot.active_story},
        {"active_state", snapshot.active_state},
        {"active_scene", snapshot.active_scene},
        {"active_timeline", snapshot.active_timeline},
        {"active_label", snapshot.active_label},
        {"variables", snapshot.variables},
        {"last_choice", snapshot.last_choice},
        {"waits", snapshot.waits},
        {"call_stack", snapshot.call_stack},
        {"random_streams", snapshot.random_streams},
        {"stage_state", snapshot.stage_state},
        {"timeline_state", snapshot.timeline_state},
        {"choice_state", snapshot.choice_state},
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

nlohmann::json ToJson(const ScriptCommandSchema& schema) {
    return {
        {"schema_id", schema.schema_id},
        {"namespace_id", schema.namespace_id},
        {"command_id", schema.command_id},
        {"version", schema.version},
        {"params", schema.params},
        {"runtime_event", schema.runtime_event},
        {"presentation", schema.presentation},
        {"state", schema.state},
        {"policy", schema.policy},
        {"channels", schema.channels},
        {"deterministic", schema.deterministic},
        {"extension", schema.extension},
    };
}

nlohmann::json ToJson(const ScriptExtensionCommandSchema& schema) {
    return {{"extension_id", schema.extension_id}, {"command_id", schema.command_id}, {"version", schema.version}, {"params", schema.params}, {"execution", schema.execution}, {"editor", schema.editor}};
}

nlohmann::json ToJson(const ScriptStepResult& result) {
    nlohmann::json events = nlohmann::json::array();
    for (const auto& event : result.events) {
        events.push_back(Astra::Runtime::ToJson(event));
    }
    nlohmann::json commands = nlohmann::json::array();
    for (const auto& command : result.presentation_commands) {
        commands.push_back(Astra::Media::ToJson(command));
    }
    return {{"paused", result.paused}, {"completed", result.completed}, {"current_command", ToJson(result.current_command)}, {"location", ToJson(result.location)}, {"variables", result.variables}, {"events", events}, {"presentation_commands", commands}};
}

nlohmann::json ToJson(const ScriptCompatibilityReport& report) {
    return {{"schema", report.schema}, {"compatible", report.compatible}, {"migrated_command_ids", report.migrated_command_ids}, {"incompatible_command_ids", report.incompatible_command_ids}, {"diagnostics", report.diagnostics}};
}

nlohmann::json ToJson(const ScriptHotReloadReport& report) {
    return {{"schema", report.schema}, {"reloaded", report.reloaded}, {"rolled_back", report.rolled_back}, {"compatibility", ToJson(report.compatibility)}};
}

} // namespace Astra::Script
