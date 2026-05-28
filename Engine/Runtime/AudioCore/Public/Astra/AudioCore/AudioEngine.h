#pragma once

#include <Astra/Bootstrap/RuntimeProviderRegistry.h>
#include <Astra/Core/Diagnostics.h>

#include <filesystem>

namespace astra {

class AudioEngine {
  public:
    explicit AudioEngine(DiagnosticSink& diagnostics);
    ~AudioEngine();

    AudioEngine(const AudioEngine&) = delete;
    AudioEngine& operator=(const AudioEngine&) = delete;

    void play_sound(const std::filesystem::path& path);
    void stop_all();
    [[nodiscard]] bool available() const;

  private:
    void* engine_ = nullptr;
    DiagnosticSink& diagnostics_;
};

void register_miniaudio_plugin(RuntimeProviderRegistry& registry, DiagnosticSink& diagnostics);

} // namespace astra
