#include <Astra/Tools/Tools.hpp>

#include <CLI/CLI.hpp>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    CLI::App app{"AstraEngine foundation CLI"};
    Astra::Tools::CommandOptions options;
    app.add_flag("--json", options.json, "Emit JSON report");
    app.add_option("--diagnostics-out", options.diagnostics_out, "Write diagnostics/report JSON");
    app.add_flag("--strict", options.strict, "Treat warnings as strict validation input where supported");
    app.add_option("--profile", options.profile, "Foundation profile");

    bool version = false;
    app.add_flag("--version", version, "Print build info");

    std::filesystem::path target;
    std::filesystem::path sample;
    std::filesystem::path inspect_target;
    std::filesystem::path run_target;
    std::filesystem::path replay_target;

    auto* doc_check = app.add_subcommand("doc-check", "Run documentation checks");
    doc_check->add_flag("--json", options.json, "Emit JSON report");
    doc_check->add_option("--diagnostics-out", options.diagnostics_out, "Write diagnostics/report JSON");
    auto* validate = app.add_subcommand("validate", "Validate repository, plugin, or foundation sample");
    validate->add_option("target", target)->required();
    validate->add_flag("--json", options.json, "Emit JSON report");
    validate->add_flag("--strict", options.strict, "Run strict foundation validation");
    validate->add_option("--diagnostics-out", options.diagnostics_out, "Write diagnostics/report JSON");
    auto* inspect = app.add_subcommand("inspect", "Inspect plugin YAML or foundation report JSON");
    inspect->add_option("target", inspect_target)->required();
    inspect->add_flag("--json", options.json, "Emit JSON report");
    inspect->add_option("--diagnostics-out", options.diagnostics_out, "Write diagnostics/report JSON");
    auto* cook = app.add_subcommand("cook", "Cook a runtime sample");
    cook->add_option("sample", sample)->required();
    cook->add_flag("--json", options.json, "Emit JSON report");
    cook->add_option("--config", options.config, "Cook/build configuration");
    cook->add_option("--profile", options.profile, "Foundation profile");
    cook->add_option("--diagnostics-out", options.diagnostics_out, "Write diagnostics/report JSON");
    auto* package = app.add_subcommand("package", "Package a runtime sample");
    package->add_option("sample", sample)->required();
    package->add_flag("--json", options.json, "Emit JSON report");
    package->add_option("--profile", options.profile, "Foundation profile");
    package->add_flag("--deterministic", options.compare, "Alias for deterministic package profile evidence");
    package->add_option("--diagnostics-out", options.diagnostics_out, "Write diagnostics/report JSON");
    auto* run = app.add_subcommand("run", "Run a headless foundation smoke");
    run->add_option("target", run_target)->required();
    run->add_flag("--json", options.json, "Emit JSON report");
    run->add_flag("--headless-smoke", options.headless_smoke, "Run the headless smoke path");
    run->add_option("--diagnostics-out", options.diagnostics_out, "Write diagnostics/report JSON");
    auto* replay = app.add_subcommand("replay", "Compare a golden runtime replay");
    replay->add_option("target", replay_target)->required();
    replay->add_flag("--json", options.json, "Emit JSON report");
    replay->add_flag("--compare", options.compare, "Compare replay hashes");
    replay->add_option("--diagnostics-out", options.diagnostics_out, "Write diagnostics/report JSON");

    CLI11_PARSE(app, argc, argv);

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
    } else if (*cook) {
        report = Astra::Tools::Cook(sample, options);
    } else if (*package) {
        report = Astra::Tools::Package(sample, options);
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
    return report.Passed() ? 0 : 1;
}
