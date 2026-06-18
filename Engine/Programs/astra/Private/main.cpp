#include <Astra/Tools/Tools.hpp>

#include <Astra/Core/Logging.hpp>

#include <CLI/CLI.hpp>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    CLI::App app{"AstraEngine foundation CLI"};
    Astra::Tools::CommandOptions options;
    app.add_flag("--json", options.json, "Emit JSON report");
    app.add_option("--diagnostics-out", options.diagnostics_out, "Write diagnostics/report JSON");
    app.add_flag("--strict", options.strict,
                 "Treat warnings as strict validation input where supported");
    app.add_option("--profile", options.profile, "Foundation profile");
    auto add_logging_options = [&](CLI::App* target) {
        target->add_option("--log-dir", options.log_dir, "Write logs under this directory");
        target->add_option("--log-file", options.log_file, "Write logs to this JSONL file");
        target->add_option("--log-level", options.log_level, "File log level");
        target->add_flag("--log-async", options.log_async, "Use async logging");
        target->add_flag("--log-sync", options.log_sync, "Use synchronous logging");
    };
    add_logging_options(&app);

    bool version = false;
    app.add_flag("--version", version, "Print build info");

    std::filesystem::path target;
    std::filesystem::path import_project;
    std::filesystem::path import_source;
    std::filesystem::path sample;
    std::filesystem::path inspect_target;
    std::filesystem::path run_target;
    std::filesystem::path replay_target;
    std::filesystem::path release_target;

    auto* doc_check = app.add_subcommand("doc-check", "Run documentation checks");
    doc_check->add_flag("--json", options.json, "Emit JSON report");
    doc_check->add_option("--diagnostics-out", options.diagnostics_out,
                          "Write diagnostics/report JSON");
    add_logging_options(doc_check);
    auto* validate =
        app.add_subcommand("validate", "Validate repository, plugin, or foundation sample");
    validate->add_option("target", target)->required();
    validate->add_flag("--json", options.json, "Emit JSON report");
    validate->add_flag("--strict", options.strict, "Run strict foundation validation");
    validate->add_option("--diagnostics-out", options.diagnostics_out,
                         "Write diagnostics/report JSON");
    add_logging_options(validate);
    auto* inspect = app.add_subcommand("inspect", "Inspect plugin YAML or foundation report JSON");
    inspect->add_option("target", inspect_target)->required();
    inspect->add_flag("--json", options.json, "Emit JSON report");
    inspect->add_option("--diagnostics-out", options.diagnostics_out,
                        "Write diagnostics/report JSON");
    add_logging_options(inspect);
    auto* import =
        app.add_subcommand("import", "Import a source asset into a project Content directory");
    import->add_option("project", import_project)->required();
    import->add_option("source", import_source)->required();
    import->add_option("--asset-id", options.import_asset_id, "Target native:/ AssetId")
        ->required();
    import->add_option("--type", options.import_asset_type, "Asset type");
    import->add_option("--preset", options.import_preset, "Import/cook preset");
    import->add_option("--license-owner", options.import_license_owner, "License owner");
    import->add_option("--license-usage", options.import_license_usage, "License usage");
    import->add_flag("--json", options.json, "Emit JSON report");
    import->add_option("--diagnostics-out", options.diagnostics_out,
                       "Write diagnostics/report JSON");
    add_logging_options(import);
    auto* cook = app.add_subcommand("cook", "Cook a runtime sample");
    cook->add_option("sample", sample)->required();
    cook->add_flag("--json", options.json, "Emit JSON report");
    cook->add_option("--config", options.config, "Cook/build configuration");
    cook->add_option("--profile", options.profile, "Foundation profile");
    cook->add_option("--diagnostics-out", options.diagnostics_out, "Write diagnostics/report JSON");
    add_logging_options(cook);
    auto* package = app.add_subcommand("package", "Package a runtime sample");
    package->add_option("sample", sample)->required();
    package->add_flag("--json", options.json, "Emit JSON report");
    package->add_option("--profile", options.profile, "Foundation profile");
    package->add_flag("--deterministic", options.compare,
                      "Alias for deterministic package profile evidence");
    package->add_option("--diagnostics-out", options.diagnostics_out,
                        "Write diagnostics/report JSON");
    add_logging_options(package);
    auto* release_gate = app.add_subcommand("release-gate", "Run runtime production release gate");
    release_gate->add_option("target", release_target)->required();
    release_gate->add_flag("--json", options.json, "Emit JSON report");
    release_gate->add_option("--profile", options.profile, "Release profile");
    release_gate->add_option("--diagnostics-out", options.diagnostics_out,
                             "Write diagnostics/report JSON");
    add_logging_options(release_gate);
    auto* run = app.add_subcommand("run", "Run a foundation/runtime smoke");
    run->add_option("target", run_target)->required();
    run->add_flag("--json", options.json, "Emit JSON report");
    run->add_flag("--headless-smoke", options.headless_smoke, "Run the headless smoke path");
    run->add_flag("--windowed-smoke", options.windowed_smoke, "Run the SDL windowed smoke path");
    run->add_flag("--gpu-smoke", options.gpu_smoke, "Run the production renderer smoke path");
    run->add_flag("--auto-close", options.auto_close,
                  "Close the windowed smoke automatically after evidence capture");
    run->add_option("--scripted-input", options.scripted_input,
                    "Scripted input YAML for smoke runs");
    run->add_option("--save-out", options.save_out, "Write a save evidence JSON file");
    run->add_option("--load", options.load,
                    "Load a save evidence JSON file before smoke verification");
    run->add_option("--diagnostics-out", options.diagnostics_out, "Write diagnostics/report JSON");
    add_logging_options(run);
    auto* replay = app.add_subcommand("replay", "Compare a golden runtime replay");
    replay->add_option("target", replay_target)->required();
    replay->add_flag("--json", options.json, "Emit JSON report");
    replay->add_flag("--compare", options.compare, "Compare replay hashes");
    replay->add_option("--diagnostics-out", options.diagnostics_out,
                       "Write diagnostics/report JSON");
    add_logging_options(replay);

    CLI11_PARSE(app, argc, argv);

    Astra::Tools::ConfigureToolLogging(options);
    Astra::Tools::CommandReport report;
    if (version) {
        report = Astra::Tools::VersionReport();
        if (!options.json) {
            const auto& info = report.build_info;
            std::cout << "AstraEngine " << info["engine_version"].get<std::string>() << "\n";
            std::cout << "git " << info["git_commit"].get<std::string>() << "\n";
            std::cout << "config " << info["build_config"].get<std::string>() << "\n";
            std::cout << "abi " << info["abi_version"].get<unsigned>() << "\n";
            std::cout << "features";
            for (const auto& feature : info["features"]) {
                std::cout << " " << feature.get<std::string>();
            }
            std::cout << "\n";
            return 0;
        }
    } else if (*doc_check) {
        report = Astra::Tools::DocCheck(options);
    } else if (*validate) {
        report = Astra::Tools::Validate(target, options);
    } else if (*inspect) {
        report = Astra::Tools::Inspect(inspect_target, options);
    } else if (*import) {
        report = Astra::Tools::Import(import_project, import_source, options);
    } else if (*cook) {
        report = Astra::Tools::Cook(sample, options);
    } else if (*package) {
        report = Astra::Tools::Package(sample, options);
    } else if (*release_gate) {
        report = Astra::Tools::ReleaseGate(release_target, options);
    } else if (*run) {
        report = Astra::Tools::Run(run_target, options);
    } else if (*replay) {
        report = Astra::Tools::Replay(replay_target, options);
    } else {
        std::cout << app.help() << "\n";
        return 0;
    }

    Astra::Tools::WriteDiagnosticsIfRequested(report, options);
    Astra::Tools::PrintReport(report, options);
    Astra::Core::FlushLogs();
    return report.Passed() ? 0 : 1;
}
