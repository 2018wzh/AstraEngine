#pragma once

#include <Astra/AstraVN/Export.hpp>
#include <Astra/Core/Diagnostics.hpp>
#include <Astra/Core/Serialization.hpp>
#include <Astra/Core/StableId.hpp>
#include <Astra/Media/Media.hpp>
#include <Astra/Runtime/Runtime.hpp>
#include <Astra/Scene/Scene.hpp>
#include <Astra/Script/Script.hpp>
#include <nlohmann/json.hpp>

#include <memory>
#include <string>
#include <vector>

namespace Astra::AstraVN {

constexpr const char* VnSessionSnapshotSchema = "astra.vn.session.snapshot.v1";

struct VnEventSchema {
    std::string category;
    std::string event_type;
    std::string payload_schema;
};

struct VnFoundationProfile {
    std::vector<VnEventSchema> event_schemas;
    std::vector<Astra::Scene::ActorDescriptor> actor_presets;
    std::vector<Astra::Scene::ComponentDescriptor> component_presets;
    std::vector<Astra::Runtime::StateMachineDefinition> state_machines;
};

struct VnSessionSnapshot {
    std::string schema = VnSessionSnapshotSchema;
    Astra::Core::u32 version = 1;
    Astra::Core::VersionedDocument runtime_save;
    Astra::Script::ScriptSnapshot script_snapshot;
    nlohmann::json route_state = nlohmann::json::object();
    nlohmann::json dialogue_history = nlohmann::json::array();
    nlohmann::json vn_events = nlohmann::json::array();
    nlohmann::json presentation_commands = nlohmann::json::array();
    Astra::Media::FrameCapture headless_capture;
    Astra::Runtime::RuntimeHashes hashes;
};

class ASTRA_ASTRAVN_API VnSession {
public:
    explicit VnSession(Astra::Core::u64 seed = 0);
    VnSession(VnSession&&) noexcept;
    VnSession& operator=(VnSession&&) noexcept;
    ~VnSession();

    VnSession(const VnSession&) = delete;
    VnSession& operator=(const VnSession&) = delete;

    [[nodiscard]] Astra::Core::Result<Astra::Script::ScriptExecutionResult> RunNative(
        const Astra::Script::ScriptSource& source,
        const Astra::Script::ScriptExecutionOptions& options,
        Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] Astra::Core::Result<Astra::Script::ScriptExecutionResult> RunLua(
        const Astra::Script::ScriptSource& source,
        const Astra::Script::ScriptExecutionOptions& options,
        Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] VnSessionSnapshot CaptureSnapshot(Astra::Core::DiagnosticSink& diagnostics) const;
    [[nodiscard]] Astra::Core::Result<void> Restore(const VnSessionSnapshot& snapshot, Astra::Core::DiagnosticSink& diagnostics);
    [[nodiscard]] Astra::Runtime::RuntimeWorld& Runtime();
    [[nodiscard]] const Astra::Runtime::RuntimeWorld& Runtime() const;

private:
    class Impl;
    std::unique_ptr<Impl> impl_;
};

[[nodiscard]] ASTRA_ASTRAVN_API std::vector<VnEventSchema> FoundationEventSchemas();
[[nodiscard]] ASTRA_ASTRAVN_API std::vector<Astra::Scene::ActorDescriptor> FoundationActorPresets();
[[nodiscard]] ASTRA_ASTRAVN_API std::vector<Astra::Scene::ComponentDescriptor> FoundationComponentPresets();
[[nodiscard]] ASTRA_ASTRAVN_API std::vector<Astra::Runtime::StateMachineDefinition> FoundationStateMachines();
[[nodiscard]] ASTRA_ASTRAVN_API VnFoundationProfile FoundationProfile();
[[nodiscard]] ASTRA_ASTRAVN_API Astra::Media::FilterProfile FoundationFilterProfile();
[[nodiscard]] ASTRA_ASTRAVN_API nlohmann::json ToJson(const VnEventSchema& schema);
[[nodiscard]] ASTRA_ASTRAVN_API nlohmann::json ToJson(const VnFoundationProfile& profile);
[[nodiscard]] ASTRA_ASTRAVN_API nlohmann::json ToJson(const VnSessionSnapshot& snapshot);

} // namespace Astra::AstraVN
