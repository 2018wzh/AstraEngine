#include <Astra/VNRuntimeServices/RuntimeServices.h>

#include <entt/entt.hpp>

#include <deque>

namespace astra {

namespace {

struct BackgroundComponent {
    std::string asset_id;
};

struct SpriteComponent {
    std::string asset_id;
    StagePosition position = StagePosition::Center;
};

struct DialogueComponent {
    std::string speaker;
    std::string text;
};

} // namespace

struct RuntimeServices::Impl {
    entt::registry world;
    RuntimeRenderSnapshot snapshot;
    std::vector<RuntimeAudioRequest> audio_requests;
    std::vector<RuntimeSchedulePhase> last_schedule;
};

RuntimeServices::RuntimeServices() : impl_(std::make_unique<Impl>()) {}

RuntimeServices::~RuntimeServices() = default;

void RuntimeServices::submit_command(const RuntimeCommand& command, DiagnosticSink&) {
    std::visit(
        [this](const auto& concrete) {
            using T = std::decay_t<decltype(concrete)>;
            if constexpr (std::is_same_v<T, ShowBackground>) {
                auto entity = impl_->world.create();
                impl_->world.emplace<BackgroundComponent>(entity, concrete.background.value());
                impl_->snapshot.background_asset = concrete.background.value();
            } else if constexpr (std::is_same_v<T, ShowCharacter>) {
                auto entity = impl_->world.create();
                impl_->world.emplace<SpriteComponent>(entity, concrete.sprite.value(),
                                                      concrete.position);
                impl_->snapshot.sprites.clear();
                impl_->snapshot.sprites.push_back({concrete.sprite.value(), concrete.position});
            } else if constexpr (std::is_same_v<T, PlayBGM>) {
                impl_->audio_requests.push_back({concrete.bgm.value(), true});
            } else if constexpr (std::is_same_v<T, PlaySFX>) {
                impl_->audio_requests.push_back({concrete.sfx.value(), false});
            } else if constexpr (std::is_same_v<T, ShowDialogue>) {
                auto entity = impl_->world.create();
                impl_->world.emplace<DialogueComponent>(entity, concrete.speaker, concrete.text);
                impl_->snapshot.speaker = concrete.speaker;
                impl_->snapshot.dialogue = concrete.text;
                impl_->snapshot.choices.clear();
            } else if constexpr (std::is_same_v<T, PresentChoice>) {
                impl_->snapshot.choices.clear();
                for (const ChoiceOption& option : concrete.options) {
                    impl_->snapshot.choices.push_back(option.text);
                }
            } else if constexpr (std::is_same_v<T, SetVariable>) {
                impl_->snapshot.variables[concrete.name] += concrete.delta;
            } else if constexpr (std::is_same_v<T, JumpScene>) {
                (void)concrete;
            }
        },
        command);
}

void RuntimeServices::run_schedule() {
    impl_->last_schedule = {RuntimeSchedulePhase::Input,        RuntimeSchedulePhase::Script,
                            RuntimeSchedulePhase::CommandApply, RuntimeSchedulePhase::Animation,
                            RuntimeSchedulePhase::Audio,        RuntimeSchedulePhase::RenderExtract,
                            RuntimeSchedulePhase::SaveSnapshot, RuntimeSchedulePhase::Cleanup};
}

RuntimeRenderSnapshot RuntimeServices::render_snapshot() const {
    return impl_->snapshot;
}

std::vector<RuntimeAudioRequest> RuntimeServices::consume_audio_requests() {
    auto requests = std::move(impl_->audio_requests);
    impl_->audio_requests.clear();
    return requests;
}

RuntimeSaveSnapshot RuntimeServices::save_snapshot() const {
    nlohmann::json world;
    world["background_asset"] = impl_->snapshot.background_asset;
    world["speaker"] = impl_->snapshot.speaker;
    world["dialogue"] = impl_->snapshot.dialogue;
    world["choices"] = impl_->snapshot.choices;
    world["variables"] = impl_->snapshot.variables;
    nlohmann::json sprites = nlohmann::json::array();
    for (const RenderSprite& sprite : impl_->snapshot.sprites) {
        sprites.push_back(
            {{"asset_id", sprite.asset_id}, {"position", static_cast<int>(sprite.position)}});
    }
    world["sprites"] = std::move(sprites);
    return RuntimeSaveSnapshot{std::move(world)};
}

void RuntimeServices::restore_snapshot(const RuntimeSaveSnapshot& snapshot) {
    impl_->snapshot = {};
    impl_->snapshot.background_asset = snapshot.world.value("background_asset", "");
    impl_->snapshot.speaker = snapshot.world.value("speaker", "");
    impl_->snapshot.dialogue = snapshot.world.value("dialogue", "");
    if (snapshot.world.contains("choices")) {
        impl_->snapshot.choices = snapshot.world["choices"].get<std::vector<std::string>>();
    }
    if (snapshot.world.contains("variables")) {
        impl_->snapshot.variables =
            snapshot.world["variables"].get<std::unordered_map<std::string, int>>();
    }
    if (snapshot.world.contains("sprites")) {
        for (const auto& sprite : snapshot.world["sprites"]) {
            impl_->snapshot.sprites.push_back(
                {sprite.value("asset_id", ""),
                 static_cast<StagePosition>(sprite.value("position", 1))});
        }
    }
}

const std::vector<RuntimeSchedulePhase>& RuntimeServices::last_schedule() const {
    return impl_->last_schedule;
}

std::string schedule_phase_to_string(RuntimeSchedulePhase phase) {
    switch (phase) {
    case RuntimeSchedulePhase::Input:
        return "Input";
    case RuntimeSchedulePhase::Script:
        return "Script";
    case RuntimeSchedulePhase::CommandApply:
        return "CommandApply";
    case RuntimeSchedulePhase::Animation:
        return "Animation";
    case RuntimeSchedulePhase::Audio:
        return "Audio";
    case RuntimeSchedulePhase::RenderExtract:
        return "RenderExtract";
    case RuntimeSchedulePhase::SaveSnapshot:
        return "SaveSnapshot";
    case RuntimeSchedulePhase::Cleanup:
        return "Cleanup";
    }
    return "Unknown";
}

} // namespace astra
