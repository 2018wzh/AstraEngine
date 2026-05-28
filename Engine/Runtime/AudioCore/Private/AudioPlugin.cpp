#include <Astra/AudioCore/AudioPlugin.h>

#include <Astra/AudioCore/AudioEngine.h>

namespace astra {

namespace {

class MiniaudioRuntime final : public IAudioRuntime {
public:
    explicit MiniaudioRuntime(DiagnosticSink& diagnostics) : audio_(diagnostics) {}

    [[nodiscard]] bool available() const override {
        return audio_.available();
    }

    void play_sound(const std::filesystem::path& path) override {
        audio_.play_sound(path);
    }

    void stop_all() override {
        audio_.stop_all();
    }

private:
    AudioEngine audio_;
};

class MiniaudioProvider final : public IAudioProvider {
public:
    [[nodiscard]] std::string id() const override {
        return "astra.audio.miniaudio";
    }

    [[nodiscard]] std::unique_ptr<IAudioRuntime>
    create_audio(DiagnosticSink& diagnostics) const override {
        auto audio = std::make_unique<MiniaudioRuntime>(diagnostics);
        if (!audio->available()) {
            diagnostics.warning("audio.unavailable", "miniaudio provider is unavailable");
        }
        return audio;
    }
};

} // namespace

void register_miniaudio_plugin(RuntimeProviderRegistry& registry, DiagnosticSink& diagnostics) {
    (void)registry.register_audio_provider(std::make_shared<MiniaudioProvider>(), diagnostics);
}

} // namespace astra
