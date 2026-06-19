#include <filesystem>
#include <string>
#include <vector>

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#include <windows.h>

std::wstring QuoteArg(const std::wstring& value) {
    std::wstring quoted = L"\"";
    for (wchar_t ch : value) {
        if (ch == L'"') {
            quoted += L'\\';
        }
        quoted += ch;
    }
    quoted += L"\"";
    return quoted;
}

int wmain(int argc, wchar_t** argv) {
    wchar_t exe_buffer[MAX_PATH]{};
    const auto length = GetModuleFileNameW(nullptr, exe_buffer, MAX_PATH);
    if (length == 0 || length == MAX_PATH) {
        return 2;
    }

    const std::filesystem::path self(exe_buffer);
    const auto root = self.parent_path();
    const auto sample = self.stem().wstring();
    const auto engine = root / L"Engine" / L"astra.exe";
    const auto package = root / L"Packages" / (sample + L".astrapkg");

    std::wstring command = QuoteArg(engine.wstring()) + L" play " + QuoteArg(package.wstring());
    for (int i = 1; i < argc; ++i) {
        command += L" ";
        command += QuoteArg(argv[i]);
    }

    STARTUPINFOW startup{};
    startup.cb = sizeof(startup);
    PROCESS_INFORMATION process{};
    if (!CreateProcessW(engine.c_str(), command.data(), nullptr, nullptr, FALSE, 0, nullptr,
                        root.c_str(), &startup, &process)) {
        return 3;
    }

    WaitForSingleObject(process.hProcess, INFINITE);
    DWORD exit_code = 0;
    GetExitCodeProcess(process.hProcess, &exit_code);
    CloseHandle(process.hThread);
    CloseHandle(process.hProcess);
    return static_cast<int>(exit_code);
}
#else
#include <sys/wait.h>
#include <unistd.h>

int main(int argc, char** argv) {
    const std::filesystem::path self = std::filesystem::absolute(argv[0]);
    const auto root = self.parent_path();
    const auto sample = self.stem().string();
    const auto engine = root / "Engine" / "astra";
    const auto package = root / "Packages" / (sample + ".astrapkg");

    std::vector<std::string> args = {engine.string(), "play", package.string()};
    for (int i = 1; i < argc; ++i) {
        args.emplace_back(argv[i]);
    }
    std::vector<char*> exec_args;
    for (auto& arg : args) {
        exec_args.push_back(arg.data());
    }
    exec_args.push_back(nullptr);
    execv(engine.c_str(), exec_args.data());
    return 3;
}
#endif
