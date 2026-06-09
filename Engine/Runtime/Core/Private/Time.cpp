#include <Astra/Core/Time.hpp>

namespace Astra::Core {

TimePoint MonotonicClock::Now() const {
    const auto now = std::chrono::steady_clock::now().time_since_epoch();
    return {.monotonic_ns = static_cast<u64>(std::chrono::duration_cast<std::chrono::nanoseconds>(now).count())};
}

FixedStepClock::FixedStepClock(double fixed_delta_seconds) {
    time_.fixed_delta_seconds = fixed_delta_seconds;
}

void FixedStepClock::Tick() {
    if (time_.paused) {
        return;
    }
    ++time_.frame_index;
    time_.elapsed_seconds += time_.fixed_delta_seconds;
}

void FixedStepClock::SetPaused(bool paused) {
    time_.paused = paused;
}

const GameTime& FixedStepClock::State() const {
    return time_;
}

} // namespace Astra::Core

