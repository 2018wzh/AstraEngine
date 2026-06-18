#pragma once

#include <Astra/Asset/Asset.hpp>
#include <Astra/Core/Diagnostics.hpp>
#include <Astra/Core/Serialization.hpp>
#include <Astra/Core/StableId.hpp>
#include <Astra/Core/Types.hpp>
#include <Astra/Media/Media.hpp>
#include <Astra/Runtime/Runtime.hpp>
#include <Astra/Script/Export.hpp>
#include <nlohmann/json.hpp>

#include <map>
#include <optional>
#include <string>
#include <vector>

namespace Astra::Script {

constexpr const char* NativeRuntimeId = "astra.script.native";
constexpr const char* LuaExtensionRuntimeId = "astra.script.extension.lua";
constexpr const char* LuaRuntimeId = LuaExtensionRuntimeId;
constexpr const char* ScriptSnapshotSchema = "astra.script.snapshot.v1";
constexpr const char* ScriptSnapshotV2Schema = "astra.script.snapshot.v2";

enum class ScriptCommandKind {
    Label,
    Story,
    State,
    Scene,
    Stage,
    Timeline,
    Background,
    ShowCharacter,
    HideCharacter,
    Pose,
    Move,
    Camera,
    Say,
    Choice,
    Jump,
    Call,
    Return,
    Emit,
    Await,
    Wait,
    End,
    SetVariable,
    LetVariable,
    IncrementVariable,
    DecrementVariable,
    ToggleVariable,
    PushVariable,
    RemoveVariable,
    ClearVariable,
    GetVariable,
    Audio,
    Filter,
    Sequence,
    Parallel,
    Race,
    Spawn,
    Cancel,
    Extension
};

struct ScriptSourceLocation {
    std::string file;
    Astra::Core::u32 line = 0;
    Astra::Core::u32 column = 0;
};

struct ScriptSource {
    std::string runtime_id = NativeRuntimeId;
    std::string source_id = "native:/Scripts/opening";
    std::string text;
    std::string file;
};

struct ScriptChoice {
    std::string stable_id;
    std::string text;
    std::string target_label;
    nlohmann::json guard = nlohmann::json::object();
};

struct ScriptCommand {
    ScriptCommandKind kind = ScriptCommandKind::Label;
    std::string command_id;
    std::string stable_id;
    ScriptSourceLocation location;
    std::string story;
    std::string state;
    std::string scene;
    std::string label;
    std::string actor;
    Astra::Asset::AssetUri asset;
    std::string placement;
    std::string text;
    std::string target_label;
    std::string schema_id;
    std::string extension_id;
    std::string extension_command;
    std::string variable_name;
    nlohmann::json value = nlohmann::json();
    std::vector<ScriptChoice> choices;
    nlohmann::json payload = nlohmann::json::object();
};

struct ScriptDebugSymbol {
    std::string command_id;
    std::string stable_id;
    ScriptSourceLocation location;
    std::string label;
    std::string node_kind;
};

struct ScriptSourceMapEntry {
    std::string command_id;
    std::string stable_id;
    ScriptSourceLocation location;
    std::string graph_node;
    std::string timeline_key;
};

struct ScriptAstNode {
    std::string stable_id;
    std::string kind;
    ScriptSourceLocation location;
    nlohmann::json fields = nlohmann::json::object();
};

struct ScriptDocument {
    std::string schema = "astra.script.document.v1";
    std::string source_id;
    std::vector<ScriptAstNode> nodes;
    nlohmann::json cst = nlohmann::json::object();
};

struct StateGraphIr {
    std::string schema = "astra.script.state_graph_ir.v1";
    nlohmann::json states = nlohmann::json::array();
    nlohmann::json scenes = nlohmann::json::array();
    nlohmann::json transitions = nlohmann::json::array();
};

struct NarrativeIr {
    std::string schema = "astra.script.narrative_ir.v1";
    nlohmann::json commands = nlohmann::json::array();
};

struct EffectGraphIr {
    std::string schema = "astra.script.effect_graph_ir.v1";
    nlohmann::json effects = nlohmann::json::array();
    nlohmann::json channels = nlohmann::json::array();
};

struct ScriptSourceMap {
    std::string schema = "astra.script.source_map.v1";
    std::vector<ScriptSourceMapEntry> entries;
};

struct ScriptBreakpoint {
    std::string command_id;
    std::string stable_id;
    ScriptSourceLocation location;
    bool enabled = true;
};

struct ScriptStepResult {
    bool paused = false;
    bool completed = false;
    ScriptCommand current_command;
    ScriptSourceLocation location;
    nlohmann::json variables = nlohmann::json::object();
    std::vector<Astra::Runtime::RuntimeEvent> events;
    std::vector<Astra::Media::PresentationCommand> presentation_commands;
};

struct ScriptCompatibilityReport {
    std::string schema = "astra.script.compatibility.v1";
    bool compatible = false;
    std::vector<std::string> migrated_command_ids;
    std::vector<std::string> incompatible_command_ids;
    nlohmann::json diagnostics = nlohmann::json::array();
};

struct ScriptHotReloadReport {
    std::string schema = "astra.script.hot_reload.v1";
    bool reloaded = false;
    bool rolled_back = false;
    ScriptCompatibilityReport compatibility;
};

struct ScriptCommandSchema {
    std::string schema_id;
    std::string namespace_id;
    std::string command_id;
    Astra::Core::u32 version = 1;
    nlohmann::json params = nlohmann::json::object();
    nlohmann::json runtime_event = nlohmann::json::object();
    nlohmann::json presentation = nlohmann::json::object();
    nlohmann::json state = nlohmann::json::object();
    nlohmann::json policy = nlohmann::json::object();
    std::vector<std::string> channels;
    bool deterministic = true;
    bool extension = false;
};

struct CompiledScript {
    std::string runtime_id;
    std::string source_id;
    ScriptDocument document;
    StateGraphIr state_graph;
    NarrativeIr narrative;
    EffectGraphIr effects;
    ScriptSourceMap source_map;
    std::vector<ScriptCommand> commands;
    std::map<std::string, Astra::Core::u32> labels;
    std::vector<ScriptDebugSymbol> debug_symbols;
    nlohmann::json command_manifest = nlohmann::json::array();
    nlohmann::json extension_manifest = nlohmann::json::array();
};

struct ScriptSnapshot {
    std::string schema = ScriptSnapshotV2Schema;
    std::string runtime_id;
    std::string source_id;
    Astra::Core::u32 current_command_index = 0;
    std::string current_command_id;
    std::string current_stable_id;
    std::string active_story;
    std::string active_state;
    std::string active_scene;
    std::string active_timeline;
    std::string active_label;
    nlohmann::json variables = nlohmann::json::object();
    std::string last_choice;
    nlohmann::json waits = nlohmann::json::array();
    nlohmann::json call_stack = nlohmann::json::array();
    nlohmann::json random_streams = nlohmann::json::object();
    nlohmann::json stage_state = nlohmann::json::object();
    nlohmann::json timeline_state = nlohmann::json::object();
    nlohmann::json choice_state = nlohmann::json::object();
    std::vector<std::string> commands_executed;
};

struct ScriptExtensionCommandSchema {
    std::string extension_id;
    std::string command_id;
    Astra::Core::u32 version = 1;
    nlohmann::json params = nlohmann::json::object();
    nlohmann::json execution = nlohmann::json::object();
    nlohmann::json editor = nlohmann::json::object();
};

struct ScriptProviderDescriptor {
    std::string runtime_id;
    std::string display_name;
    bool debug_supported = false;
    bool snapshot_supported = true;
    bool deterministic = true;
    bool packaged_eligible = true;
    std::vector<std::string> source_extensions;
    std::vector<std::string> capabilities;
    std::vector<std::string> permissions;
};

struct ScriptExecutionOptions {
    std::string entry_label = "opening";
    Astra::Core::u32 choice_index = 0;
};

struct ScriptExecutionResult {
    std::vector<Astra::Runtime::RuntimeEvent> events;
    std::vector<Astra::Media::PresentationCommand> presentation_commands;
    ScriptSnapshot snapshot;
};

class ASTRA_SCRIPT_API ScriptEventBridge {
public:
    [[nodiscard]] ScriptExecutionResult Execute(
        const CompiledScript& script,
        Astra::Runtime::RuntimeWorld& runtime,
        const ScriptExecutionOptions& options,
        Astra::Core::DiagnosticSink& diagnostics) const;
};

class ASTRA_SCRIPT_API ScriptRuntimeHost {
public:
    ScriptRuntimeHost();

    [[nodiscard]] const std::vector<ScriptProviderDescriptor>& Providers() const;
    [[nodiscard]] Astra::Core::Result<CompiledScript> CompileNative(const ScriptSource& source, Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] Astra::Core::Result<CompiledScript> CompileLua(const ScriptSource& source, Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] Astra::Core::Result<std::vector<ScriptExtensionCommandSchema>> CompileLuaExtensionPackage(const ScriptSource& source, Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] const std::vector<ScriptCommandSchema>& CommandSchemas() const;
    [[nodiscard]] ScriptExecutionResult Run(
        const CompiledScript& script,
        Astra::Runtime::RuntimeWorld& runtime,
        const ScriptExecutionOptions& options,
        Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] ScriptStepResult Step(
        const CompiledScript& script,
        const ScriptSnapshot& snapshot,
        Astra::Runtime::RuntimeWorld& runtime,
        Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] ScriptCompatibilityReport CheckHotReloadCompatibility(const CompiledScript& old_script, const CompiledScript& new_script, const ScriptSnapshot& snapshot) const;
    [[nodiscard]] ScriptHotReloadReport HotReload(const CompiledScript& old_script, const CompiledScript& new_script, const ScriptSnapshot& snapshot) const;

private:
    std::vector<ScriptProviderDescriptor> providers_;
    std::vector<ScriptCommandSchema> command_schemas_;
    ScriptEventBridge bridge_;
};

[[nodiscard]] ASTRA_SCRIPT_API std::vector<ScriptProviderDescriptor> FoundationScriptProviders();
[[nodiscard]] ASTRA_SCRIPT_API std::vector<ScriptCommandSchema> FoundationCommandSchemas();
[[nodiscard]] ASTRA_SCRIPT_API std::string ToString(ScriptCommandKind kind);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptSourceLocation& location);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptChoice& choice);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptCommand& command);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptDebugSymbol& symbol);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptSourceMapEntry& entry);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptDocument& document);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const StateGraphIr& ir);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const NarrativeIr& ir);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const EffectGraphIr& ir);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptSourceMap& source_map);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const CompiledScript& script);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptSnapshot& snapshot);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptProviderDescriptor& descriptor);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptExecutionResult& result);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptCommandSchema& schema);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptExtensionCommandSchema& schema);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptStepResult& result);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptCompatibilityReport& report);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptHotReloadReport& report);

} // namespace Astra::Script
