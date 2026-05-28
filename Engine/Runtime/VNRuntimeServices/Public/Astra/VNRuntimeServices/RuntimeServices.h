#pragma once

#include <Astra/Core/Diagnostics.h>
#include <Astra/VNRuntimeServices/RuntimeCommand.h>

#include <memory>
#include <string>
#include <vector>

namespace astra {

enum class RuntimeSchedulePhase {
    Input,
    Script,
    CommandApply,
    Animation,
    Audio,
    RenderExtract,
    SaveSnapshot,
    Cleanup,
};

class RuntimeServices {
  public:
    RuntimeServices();
    ~RuntimeServices();

    RuntimeServices(const RuntimeServices&) = delete;
    RuntimeServices& operator=(const RuntimeServices&) = delete;

    void submit_command(const RuntimeCommand& command, DiagnosticSink& diagnostics);
    void run_schedule();
    [[nodiscard]] RuntimeRenderSnapshot render_snapshot() const;
    [[nodiscard]] std::vector<RuntimeAudioRequest> consume_audio_requests();
    [[nodiscard]] RuntimeSaveSnapshot save_snapshot() const;
    void restore_snapshot(const RuntimeSaveSnapshot& snapshot);
    [[nodiscard]] const std::vector<RuntimeSchedulePhase>& last_schedule() const;

  private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

std::string schedule_phase_to_string(RuntimeSchedulePhase phase);

} // namespace astra
