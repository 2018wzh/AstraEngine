#pragma once

#include <Astra/Script/Script.hpp>

#include <string_view>

namespace Astra::Script::Private {

[[nodiscard]] std::string Trim(std::string_view text);
[[nodiscard]] bool StartsWith(std::string_view text, std::string_view prefix);
[[nodiscard]] std::vector<std::string> SplitWords(std::string_view text);

[[nodiscard]] Astra::Core::Diagnostic MakeDiagnostic(
    std::string code,
    Astra::Core::DiagnosticSeverity severity,
    std::string message,
    const ScriptSourceLocation& location,
    std::string fix = {});

void EmitBlocking(Astra::Core::DiagnosticSink& diagnostics, const ScriptSourceLocation& location, std::string code, std::string message, std::string fix = {});

[[nodiscard]] ScriptCommand MakeCommand(ScriptCommandKind kind, const ScriptSource& source, Astra::Core::u32 line, Astra::Core::u32 index);
void AppendCommand(CompiledScript& script, ScriptCommand command, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] bool ValidateCompiledScript(const CompiledScript& script, Astra::Core::DiagnosticSink& diagnostics);
[[nodiscard]] Astra::Core::EventTypeId EventType(std::string_view value);
[[nodiscard]] Astra::Runtime::RuntimeEvent MakeEvent(const ScriptCommand& command, std::string event_value, std::string category, std::string target, nlohmann::json payload);
[[nodiscard]] const ScriptCommandSchema* FindCommandSchema(std::string_view schema_id);
[[nodiscard]] std::string SchemaIdForCommand(const ScriptCommand& command);
void BindCommandSchema(ScriptCommand& command, Astra::Core::DiagnosticSink& diagnostics);
void AddPresentationForCommand(const ScriptCommand& command, std::vector<Astra::Media::PresentationCommand>& output, Astra::Core::u64 frame_index);
void AddIrForCommand(CompiledScript& script, const ScriptCommand& command);

} // namespace Astra::Script::Private
