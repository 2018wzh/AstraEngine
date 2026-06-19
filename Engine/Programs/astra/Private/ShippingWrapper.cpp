#include <Astra/AstraGame/AstraGame.hpp>
#include <Astra/Core/Diagnostics.hpp>

#include <filesystem>
#include <iostream>
#include <string>
#include <vector>

namespace {

struct Args {
    bool json = false;
    bool auto_close = false;
    std::string backend = "sdl";
};

Args ParseArgs(int argc, char** argv) {
    Args args;
    for (int index = 1; index < argc; ++index) {
        const std::string value = argv[index];
        if (value == "--json") {
            args.json = true;
        } else if (value == "--auto-close") {
            args.auto_close = true;
        } else if (value == "--backend" && index + 1 < argc) {
            args.backend = argv[++index];
        }
    }
    return args;
}

int RunLauncher(const std::filesystem::path& executable, int argc, char** argv) {
    const auto args = ParseArgs(argc, argv);
    const auto root = executable.parent_path();
    const auto sample = executable.stem().string();
    Astra::Game::GameLaunchDesc desc;
    desc.package_path = root / "Packages" / (sample + ".astrapkg");
    desc.backend = Astra::Game::GameBackendFromString(args.backend);
    desc.auto_close = args.auto_close;

    Astra::Core::DiagnosticSink diagnostics;
    Astra::Game::GameSession session;
    auto launched = session.Launch(std::move(desc), diagnostics);
    auto report = launched ? session.Run(diagnostics)
                           : Astra::Core::Result<Astra::Game::GameRunReport>::Failure(
                                 launched.Error(), launched.Message());
    if (report && args.json) {
        std::cout << Astra::Game::ToJson(report.Value()).dump(2) << "\n";
    } else if (report) {
        std::cout << report.Value().status << "\n";
    } else {
        std::cout << "failed\n";
    }
    return report && report.Value().status == "passed" ? 0 : 1;
}

} // namespace

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#include <windows.h>

int wmain(int argc, wchar_t** argv) {
    wchar_t exe_buffer[MAX_PATH]{};
    const auto length = GetModuleFileNameW(nullptr, exe_buffer, MAX_PATH);
    if (length == 0 || length == MAX_PATH) {
        return 2;
    }
    std::vector<std::string> utf8_args;
    std::vector<char*> raw_args;
    utf8_args.reserve(static_cast<std::size_t>(argc));
    raw_args.reserve(static_cast<std::size_t>(argc));
    for (int index = 0; index < argc; ++index) {
        const int size = WideCharToMultiByte(CP_UTF8, 0, argv[index], -1, nullptr, 0, nullptr, nullptr);
        std::string converted(static_cast<std::size_t>(size > 0 ? size - 1 : 0), '\0');
        if (size > 1) {
            WideCharToMultiByte(CP_UTF8, 0, argv[index], -1, converted.data(), size, nullptr, nullptr);
        }
        utf8_args.push_back(std::move(converted));
        raw_args.push_back(utf8_args.back().data());
    }
    return RunLauncher(std::filesystem::path(exe_buffer), argc, raw_args.data());
}
#else
int main(int argc, char** argv) {
    return RunLauncher(std::filesystem::absolute(argv[0]), argc, argv);
}
#endif
