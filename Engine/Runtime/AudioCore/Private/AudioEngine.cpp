#include <Astra/AudioCore/AudioEngine.h>

#include <Astra/Core/Path.h>

#define MINIAUDIO_IMPLEMENTATION
#include <miniaudio.h>

namespace astra {

AudioEngine::AudioEngine(DiagnosticSink& diagnostics) : diagnostics_(diagnostics) {
    auto* engine = new ma_engine{};
    if (ma_engine_init(nullptr, engine) != MA_SUCCESS) {
        diagnostics_.warning("audio.init_failed", "miniaudio engine could not initialize");
        delete engine;
        return;
    }
    engine_ = engine;
}

AudioEngine::~AudioEngine() {
    if (engine_ != nullptr) {
        ma_engine_uninit(static_cast<ma_engine*>(engine_));
        delete static_cast<ma_engine*>(engine_);
    }
}

void AudioEngine::play_sound(const std::filesystem::path& path) {
    if (engine_ == nullptr) {
        return;
    }
    if (!std::filesystem::exists(path)) {
        diagnostics_.warning("audio.asset_missing", "Audio source missing: " + path_to_utf8(path));
        return;
    }
    if (ma_engine_play_sound(static_cast<ma_engine*>(engine_), path_to_utf8(path).c_str(),
                             nullptr) != MA_SUCCESS) {
        diagnostics_.warning("audio.play_failed", "Failed to play sound: " + path_to_utf8(path));
    }
}

void AudioEngine::stop_all() {
    if (engine_ != nullptr) {
        ma_engine_stop(static_cast<ma_engine*>(engine_));
    }
}

bool AudioEngine::available() const {
    return engine_ != nullptr;
}

} // namespace astra
