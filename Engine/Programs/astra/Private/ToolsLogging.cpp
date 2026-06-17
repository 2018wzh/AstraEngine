#include <Astra/Tools/Tools.hpp>

#include <Astra/Core/Logging.hpp>

namespace Astra::Tools {

namespace {

std::filesystem::path BinaryRoot() {
#if defined(ASTRA_BINARY_ROOT)
    return ASTRA_BINARY_ROOT;
#else
    return std::filesystem::current_path() / "build";
#endif
}

std::filesystem::path DefaultLogDir(const CommandOptions& options) {
    if (!options.log_dir.empty()) {
        return options.log_dir;
    }
    return BinaryRoot() / "Saved/Logs";
}

} // namespace

void ConfigureToolLogging(const CommandOptions& options) {
    const bool async_enabled = !options.log_sync && options.log_async;
    Astra::Core::LogConfig config;
    config.log_directory = DefaultLogDir(options);
    config.log_file = options.log_file;
    config.file_level = Astra::Core::LogLevelFromString(options.log_level);
    config.console_level = Astra::Core::LogLevel::Info;
    config.async = async_enabled;
    config.console_enabled = true;
    config.file_enabled = true;
    Astra::Core::ConfigureLogging(std::move(config));
    Astra::Core::DefaultLogger().Log(
        "tools.lifecycle",
        "astra",
        Astra::Core::LogLevel::Info,
        "logging configured",
        {{"log_dir", DefaultLogDir(options).string()},
         {"log_file", options.log_file.string()},
         {"file_level", options.log_level},
         {"async", async_enabled ? "true" : "false"}});
}

} // namespace Astra::Tools
