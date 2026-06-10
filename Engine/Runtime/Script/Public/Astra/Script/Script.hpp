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
constexpr const char* LuaRuntimeId = "astra.script.lua";
constexpr const char* ScriptSnapshotSchema = "astra.script.snapshot.v1";

enum class ScriptCommandKind {
    Label,
    Background,
    ShowCharacter,
    Say,
    Choice,
    Jump,
    SetVariable,
    GetVariable,
    Audio,
    Filter
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
    std::string text;
    std::string target_label;
};

struct ScriptCommand {
    ScriptCommandKind kind = ScriptCommandKind::Label;
    std::string command_id;
    ScriptSourceLocation location;
    std::string label;
    std::string actor;
    Astra::Asset::AssetUri asset;
    std::string placement;
    std::string text;
    std::string target_label;
    std::string variable_name;
    nlohmann::json value = nlohmann::json();
    std::vector<ScriptChoice> choices;
    nlohmann::json payload = nlohmann::json::object();
};

struct ScriptDebugSymbol {
    std::string command_id;
    ScriptSourceLocation location;
    std::string label;
};

struct CompiledScript {
    std::string runtime_id;
    std::string source_id;
    std::vector<ScriptCommand> commands;
    std::map<std::string, Astra::Core::u32> labels;
    std::vector<ScriptDebugSymbol> debug_symbols;
};

struct ScriptSnapshot {
    std::string schema = ScriptSnapshotSchema;
    std::string runtime_id;
    std::string source_id;
    Astra::Core::u32 current_command_index = 0;
    std::string active_label;
    nlohmann::json variables = nlohmann::json::object();
    std::string last_choice;
    std::vector<std::string> commands_executed;
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
    [[nodiscard]] ScriptExecutionResult Run(
        const CompiledScript& script,
        Astra::Runtime::RuntimeWorld& runtime,
        const ScriptExecutionOptions& options,
        Astra::Core::DiagnosticSink& diagnostics) const;

private:
    std::vector<ScriptProviderDescriptor> providers_;
    ScriptEventBridge bridge_;
};

[[nodiscard]] ASTRA_SCRIPT_API std::vector<ScriptProviderDescriptor> FoundationScriptProviders();
[[nodiscard]] ASTRA_SCRIPT_API std::string ToString(ScriptCommandKind kind);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptSourceLocation& location);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptChoice& choice);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptCommand& command);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptDebugSymbol& symbol);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const CompiledScript& script);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptSnapshot& snapshot);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptProviderDescriptor& descriptor);
[[nodiscard]] ASTRA_SCRIPT_API nlohmann::json ToJson(const ScriptExecutionResult& result);

} // namespace Astra::Script
