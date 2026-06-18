#include "ScriptPrivate.hpp"

#include <Astra/Core/Logging.hpp>

#include <sol/sol.hpp>

namespace Astra::Script {

namespace {

nlohmann::json SolToJson(const sol::object& object) {
    if (object.is<bool>()) {
        return object.as<bool>();
    }
    if (object.is<int>()) {
        return object.as<int>();
    }
    if (object.is<double>()) {
        return object.as<double>();
    }
    if (object.is<std::string>()) {
        return object.as<std::string>();
    }
    if (object.is<sol::table>()) {
        nlohmann::json out = nlohmann::json::object();
        sol::table table = object.as<sol::table>();
        for (const auto& [key, value] : table) {
            if (key.is<std::string>()) {
                out[key.as<std::string>()] = SolToJson(value);
            } else if (key.is<int>()) {
                if (!out.is_array()) {
                    out = nlohmann::json::array();
                }
                out.push_back(SolToJson(value));
            }
        }
        return out;
    }
    return nullptr;
}

} // namespace

Astra::Core::Result<std::vector<ScriptExtensionCommandSchema>> ScriptRuntimeHost::CompileLuaExtensionPackage(const ScriptSource& source, Astra::Core::DiagnosticSink& diagnostics) const {
    std::vector<ScriptExtensionCommandSchema> schemas;
    try {
        sol::state lua;
        lua.open_libraries(sol::lib::base, sol::lib::table, sol::lib::string, sol::lib::math);
        lua["io"] = sol::nil;
        lua["os"] = sol::nil;
        lua["package"] = sol::nil;
        lua["debug"] = sol::nil;

        std::string active_extension;
        sol::table aivn = lua.create_table();
        aivn.set_function("extension", [&](const std::string& id, const sol::optional<std::string>&) {
            active_extension = id;
            return true;
        });
        aivn.set_function("command", [&](const std::string& command_id, sol::table descriptor) {
            if (active_extension.empty()) {
                Private::EmitBlocking(diagnostics, {source.file.empty() ? source.source_id : source.file, 1, 1}, "ASTRA_SCRIPT_LUA_EXTENSION_REQUIRED", "Lua command schema must be registered after aivn.extension().", "Call aivn.extension(\"live2d\", \"1.0.0\") first.");
                return;
            }
            ScriptExtensionCommandSchema schema;
            schema.extension_id = active_extension;
            schema.command_id = command_id;
            schema.version = descriptor.get_or("version", 1);
            schema.params = SolToJson(descriptor.get<sol::object>("params"));
            schema.execution = SolToJson(descriptor.get<sol::object>("execution"));
            schema.editor = SolToJson(descriptor.get<sol::object>("editor"));
            if (!schema.execution.value("deterministic", true)) {
                Private::EmitBlocking(diagnostics, {source.file.empty() ? source.source_id : source.file, 1, 1}, "ASTRA_SCRIPT_EXTENSION_NONDETERMINISTIC", "Extension command schema must be deterministic for packaged VN scripts.", "Declare deterministic=true or keep the command out of deterministic profiles.");
            }
            schemas.push_back(std::move(schema));
        });
        lua["aivn"] = aivn;
        lua.script(source.text);
    } catch (const sol::error& error) {
        Private::EmitBlocking(diagnostics, {source.file.empty() ? source.source_id : source.file, 1, 1}, "ASTRA_SCRIPT_LUA_SANDBOX_ERROR", error.what(), "Use only the aivn extension schema SDK in Phase 8 Lua.");
    }

    if (schemas.empty() && !diagnostics.HasBlocking()) {
        Private::EmitBlocking(diagnostics, {source.file.empty() ? source.source_id : source.file, 1, 1}, "ASTRA_SCRIPT_LUA_SCHEMA_EMPTY", "Lua extension package did not register any command schemas.", "Call aivn.command().");
    }
    if (diagnostics.HasBlocking()) {
        return Astra::Core::Result<std::vector<ScriptExtensionCommandSchema>>::Failure(Astra::Core::ErrorCode::InvalidFormat, "lua extension schema compilation failed");
    }
    return Astra::Core::Result<std::vector<ScriptExtensionCommandSchema>>::Success(std::move(schemas));
}

Astra::Core::Result<CompiledScript> ScriptRuntimeHost::CompileLua(const ScriptSource& source, Astra::Core::DiagnosticSink& diagnostics) const {
    Private::EmitBlocking(
        diagnostics,
        {source.file.empty() ? source.source_id : source.file, 1, 1},
        "ASTRA_SCRIPT_LUA_STORY_REMOVED",
        "Lua is no longer a VN story runtime. Use .astra for story scripts and CompileLuaExtensionPackage() for extension schemas.",
        "Rename the asset to an extension package or move story flow into .astra.");
    return Astra::Core::Result<CompiledScript>::Failure(Astra::Core::ErrorCode::Unsupported, "lua story scripts are removed");
}

} // namespace Astra::Script
