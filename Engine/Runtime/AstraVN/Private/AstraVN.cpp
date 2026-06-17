#include <Astra/AstraVN/AstraVN.hpp>

#include <Astra/Core/Logging.hpp>

#include <algorithm>

namespace Astra::AstraVN {

namespace {

Astra::Core::StableId Id(std::string_view text) {
    auto id = Astra::Core::ParseStableId(text);
    return id ? id.Value() : Astra::Core::StableId();
}

Astra::Scene::ComponentData Component(std::string_view id, std::string type_id, nlohmann::json data) {
    return {Id(id), std::move(type_id), 1, std::move(data)};
}

Astra::Scene::ActorDescriptor Actor(std::string_view id, std::string_view type, std::string name, std::vector<Astra::Scene::ComponentData> components = {}) {
    return {Id(id), Id(type), std::move(name), std::move(components)};
}

void AppendHistory(VnSessionSnapshot& snapshot, const Astra::Runtime::RuntimeEvent& event) {
    if (event.category == "vn.dialogue") {
        snapshot.dialogue_history.push_back(event.payload);
    }
    if (event.category == "vn.choice") {
        snapshot.route_state["last_choice_event"] = event.payload;
    }
}

} // namespace

class VnSession::Impl {
public:
    explicit Impl(Astra::Core::u64 seed) : runtime(seed) {
        SpawnPresets();
        for (auto definition : FoundationStateMachines()) {
            runtime.RegisterStateMachine(std::move(definition));
        }
    }

    void SpawnPresets() {
        Astra::Core::DiagnosticSink ignored;
        for (const auto& actor : FoundationActorPresets()) {
            auto handle = runtime.Scene().Spawn(actor, ignored);
            if (handle) {
                (void)runtime.Scene().Activate(handle.Value(), ignored);
            }
        }
    }

    Astra::Core::Result<Astra::Script::ScriptExecutionResult> Run(
        const Astra::Script::ScriptSource& source,
        const Astra::Script::ScriptExecutionOptions& options,
        bool lua,
        Astra::Core::DiagnosticSink& diagnostics) {
        Astra::Core::DefaultLogger().Log(
            "astravn.session",
            lua ? "lua" : "native",
            Astra::Core::LogLevel::Info,
            "vn session run started",
            {{"source", source.source_id}, {"entry_label", options.entry_label}});
        auto compiled = lua ? host.CompileLua(source, diagnostics) : host.CompileNative(source, diagnostics);
        if (!compiled) {
            return Astra::Core::Result<Astra::Script::ScriptExecutionResult>::Failure(compiled.Error(), compiled.Message());
        }
        last_result = host.Run(compiled.Value(), runtime, options, diagnostics);
        Astra::Core::DefaultLogger().Log(
            "astravn.session",
            lua ? "lua" : "native",
            Astra::Core::LogLevel::Info,
            "vn session run finished",
            {{"source", source.source_id},
             {"events", std::to_string(last_result.events.size())},
             {"presentation_commands", std::to_string(last_result.presentation_commands.size())}});
        return Astra::Core::Result<Astra::Script::ScriptExecutionResult>::Success(last_result);
    }

    Astra::Runtime::RuntimeWorld runtime;
    Astra::Script::ScriptRuntimeHost host;
    Astra::Script::ScriptExecutionResult last_result;
};

VnSession::VnSession(Astra::Core::u64 seed) : impl_(std::make_unique<Impl>(seed)) {}
VnSession::VnSession(VnSession&&) noexcept = default;
VnSession& VnSession::operator=(VnSession&&) noexcept = default;
VnSession::~VnSession() = default;

Astra::Core::Result<Astra::Script::ScriptExecutionResult> VnSession::RunNative(
    const Astra::Script::ScriptSource& source,
    const Astra::Script::ScriptExecutionOptions& options,
    Astra::Core::DiagnosticSink& diagnostics) {
    return impl_->Run(source, options, false, diagnostics);
}

Astra::Core::Result<Astra::Script::ScriptExecutionResult> VnSession::RunLua(
    const Astra::Script::ScriptSource& source,
    const Astra::Script::ScriptExecutionOptions& options,
    Astra::Core::DiagnosticSink& diagnostics) {
    return impl_->Run(source, options, true, diagnostics);
}

VnSessionSnapshot VnSession::CaptureSnapshot(Astra::Core::DiagnosticSink& diagnostics) const {
    Astra::Core::DefaultLogger().Log(
        "astravn.session",
        "snapshot",
        Astra::Core::LogLevel::Debug,
        "vn snapshot capture started");
    VnSessionSnapshot snapshot;
    snapshot.runtime_save = impl_->runtime.Save();
    snapshot.script_snapshot = impl_->last_result.snapshot;
    for (const auto& event : impl_->last_result.events) {
        snapshot.vn_events.push_back(Astra::Runtime::ToJson(event));
        AppendHistory(snapshot, event);
    }
    for (const auto& command : impl_->last_result.presentation_commands) {
        snapshot.presentation_commands.push_back(Astra::Media::ToJson(command));
    }

    auto profile = FoundationFilterProfile();
    auto graph = Astra::Media::ExtractRenderGraph(impl_->last_result.presentation_commands, &profile, diagnostics);
    auto renderer = Astra::Media::CreateHeadlessRenderer2D();
    renderer->Submit(std::move(graph), diagnostics);
    snapshot.headless_capture = renderer->Capture();
    snapshot.hashes = impl_->runtime.Hashes();
    Astra::Core::DefaultLogger().Log(
        "astravn.session",
        "snapshot",
        Astra::Core::LogLevel::Debug,
        "vn snapshot capture finished",
        {{"events", std::to_string(snapshot.vn_events.size())},
         {"presentation_commands", std::to_string(snapshot.presentation_commands.size())},
         {"state_hash", snapshot.hashes.state_hash}});
    return snapshot;
}

Astra::Core::Result<void> VnSession::Restore(const VnSessionSnapshot& snapshot, Astra::Core::DiagnosticSink& diagnostics) {
    Astra::Core::DefaultLogger().Log(
        "astravn.session",
        "snapshot",
        Astra::Core::LogLevel::Debug,
        "vn snapshot restore started",
        {{"events", std::to_string(snapshot.vn_events.size())}});
    impl_->last_result = {};
    impl_->last_result.snapshot = snapshot.script_snapshot;
    auto loaded = impl_->runtime.Load(snapshot.runtime_save, diagnostics);
    if (!loaded) {
        return loaded;
    }
    Astra::Core::DefaultLogger().Log(
        "astravn.session",
        "snapshot",
        Astra::Core::LogLevel::Debug,
        "vn snapshot restore finished");
    return Astra::Core::Result<void>::Success();
}

Astra::Runtime::RuntimeWorld& VnSession::Runtime() {
    return impl_->runtime;
}

const Astra::Runtime::RuntimeWorld& VnSession::Runtime() const {
    return impl_->runtime;
}

std::vector<VnEventSchema> FoundationEventSchemas() {
    return {
        {"VN.Background", "event:/astra.vn.background.show", "astra.vn.background.v1"},
        {"VN.Character", "event:/astra.vn.character.show", "astra.vn.character.v1"},
        {"VN.Dialogue", "event:/astra.vn.dialogue.say_requested", "astra.vn.dialogue.v1"},
        {"VN.Choice", "event:/astra.vn.choice.presented", "astra.vn.choice.v1"},
        {"VN.Audio", "event:/astra.vn.audio.play", "astra.vn.audio.v1"},
        {"VN.Filter", "event:/astra.vn.filter.apply", "astra.vn.filter.v1"},
        {"VN.Camera", "event:/astra.vn.camera.foundation", "astra.vn.camera.v1"},
        {"VN.Timeline", "event:/astra.vn.timeline.foundation", "astra.vn.timeline.v1"},
    };
}

std::vector<Astra::Scene::ActorDescriptor> FoundationActorPresets() {
    return {
        Actor("actor:/systems/scene", "type:/astra.vn.scene", "Scene", {Component("component:/astra.vn.scene.state", "astra.vn.scene_state", {{"background", ""}})}),
        Actor("actor:/systems/story_director", "type:/astra.vn.story_director", "Story Director", {Component("component:/astra.state_machine", "astra.state_machine", {{"state_machine_id", "state_machine:/astra.vn.story"}, {"current_state", "idle"}})}),
        Actor("actor:/systems/dialogue", "type:/astra.vn.dialogue_system", "Dialogue System", {Component("component:/astra.state_machine", "astra.state_machine", {{"state_machine_id", "state_machine:/astra.vn.dialogue"}, {"current_state", "idle"}})}),
        Actor("actor:/systems/choice", "type:/astra.vn.choice_system", "Choice System", {Component("component:/astra.state_machine", "astra.state_machine", {{"state_machine_id", "state_machine:/astra.vn.choice"}, {"current_state", "idle"}})}),
        Actor("actor:/systems/audio", "type:/astra.vn.audio_system", "Audio System", {Component("component:/astra.state_machine", "astra.state_machine", {{"state_machine_id", "state_machine:/astra.vn.audio"}, {"current_state", "idle"}})}),
        Actor("actor:/systems/filter", "type:/astra.vn.filter_system", "Filter System", {Component("component:/astra.state_machine", "astra.state_machine", {{"state_machine_id", "state_machine:/astra.vn.filter"}, {"current_state", "idle"}})}),
        Actor("actor:/characters/alice", "type:/astra.vn.character", "Alice", {Component("component:/astra.vn.character.profile", "astra.vn.character_profile", {{"display_name", "Alice"}, {"emotion", "normal"}})}),
        Actor("actor:/systems/camera", "type:/astra.vn.camera", "Camera", {Component("component:/astra.vn.camera", "astra.vn.camera", {{"mode", "foundation"}})}),
    };
}

std::vector<Astra::Scene::ComponentDescriptor> FoundationComponentPresets() {
    return {
        {Id("component:/astra.vn.character.profile"), "astra.vn.character_profile", 1, {{"display_name", ""}, {"emotion", "normal"}}, false},
        {Id("component:/astra.vn.dialogue.participant"), "astra.vn.dialogue_participant", 1, {{"speaker_id", ""}}, false},
        {Id("component:/astra.vn.choice.list"), "astra.vn.choice_list", 1, {{"choices", nlohmann::json::array()}}, false},
        {Id("component:/astra.vn.audio.cue"), "astra.vn.audio_cue", 1, {{"bus", "voice"}, {"asset", ""}}, false},
        {Id("component:/astra.vn.camera"), "astra.vn.camera", 1, {{"mode", "foundation"}}, false},
        {Id("component:/astra.vn.timeline"), "astra.vn.timeline", 1, {{"cursor", 0}}, false},
        {Id("component:/astra.vn.filter.profile"), "astra.vn.filter_profile", 1, {{"profile", ""}}, false},
    };
}

std::vector<Astra::Runtime::StateMachineDefinition> FoundationStateMachines() {
    return {
        {Id("state_machine:/astra.vn.story"), "idle", {{"idle", Id("event:/astra.vn.background.show"), "presenting"}}},
        {Id("state_machine:/astra.vn.dialogue"), "idle", {{"idle", Id("event:/astra.vn.dialogue.say_requested"), "typing"}, {"typing", Id("event:/astra.vn.choice.presented"), "waiting_choice"}}},
        {Id("state_machine:/astra.vn.choice"), "idle", {{"idle", Id("event:/astra.vn.choice.presented"), "open"}}},
        {Id("state_machine:/astra.vn.audio"), "idle", {{"idle", Id("event:/astra.vn.audio.play"), "playing"}}},
        {Id("state_machine:/astra.vn.filter"), "idle", {{"idle", Id("event:/astra.vn.filter.apply"), "applied"}}},
        {Id("state_machine:/astra.vn.character_presentation"), "idle", {{"idle", Id("event:/astra.vn.character.show"), "visible"}}},
    };
}

VnFoundationProfile FoundationProfile() {
    return {FoundationEventSchemas(), FoundationActorPresets(), FoundationComponentPresets(), FoundationStateMachines()};
}

Astra::Media::FilterProfile FoundationFilterProfile() {
    Astra::Media::FilterProfile profile;
    auto id = Astra::Asset::ParseAssetUri("native:/Filters/soft_vn");
    if (id) {
        profile.id = id.Value();
    }
    profile.passes.push_back({"bg_soften", "astra.filter.foundation_hash", Astra::Media::FilterTarget::Background, {{"strength", 0.25}}});
    profile.passes.push_back({"character_line", "astra.filter.foundation_hash", Astra::Media::FilterTarget::Character, {{"strength", 0.5}}});
    profile.passes.push_back({"final_grade", "astra.filter.foundation_hash", Astra::Media::FilterTarget::Final, {{"grade", "warm"}}});
    return profile;
}

nlohmann::json ToJson(const VnEventSchema& schema) {
    return {{"category", schema.category}, {"event_type", schema.event_type}, {"payload_schema", schema.payload_schema}};
}

nlohmann::json ToJson(const VnFoundationProfile& profile) {
    nlohmann::json events = nlohmann::json::array();
    for (const auto& event : profile.event_schemas) {
        events.push_back(ToJson(event));
    }
    nlohmann::json actors = nlohmann::json::array();
    for (const auto& actor : profile.actor_presets) {
        actors.push_back(Astra::Scene::ToJson(Astra::Scene::ActorSnapshot{actor.id, actor.type_id, actor.name, 0, Astra::Scene::ActorLifecycleState::Draft, false, actor.components}));
    }
    nlohmann::json components = nlohmann::json::array();
    for (const auto& component : profile.component_presets) {
        components.push_back(Astra::Scene::ToJson(component));
    }
    nlohmann::json machines = nlohmann::json::array();
    for (const auto& machine : profile.state_machines) {
        nlohmann::json transitions = nlohmann::json::array();
        for (const auto& transition : machine.transitions) {
            transitions.push_back({{"from", transition.from_state}, {"event", transition.event_type.ToString()}, {"to", transition.to_state}});
        }
        machines.push_back({{"id", machine.id.ToString()}, {"initial_state", machine.initial_state}, {"transitions", transitions}});
    }
    return {{"event_schemas", events}, {"actor_presets", actors}, {"component_presets", components}, {"state_machines", machines}};
}

nlohmann::json ToJson(const VnSessionSnapshot& snapshot) {
    return {
        {"schema", snapshot.schema},
        {"version", snapshot.version},
        {"runtime_save", Astra::Core::ToJson(snapshot.runtime_save)},
        {"script_snapshot", Astra::Script::ToJson(snapshot.script_snapshot)},
        {"route_state", snapshot.route_state},
        {"dialogue_history", snapshot.dialogue_history},
        {"vn_events", snapshot.vn_events},
        {"presentation_commands", snapshot.presentation_commands},
        {"headless_capture", Astra::Media::ToJson(snapshot.headless_capture)},
        {"hashes", Astra::Runtime::ToJson(snapshot.hashes)},
    };
}

} // namespace Astra::AstraVN
