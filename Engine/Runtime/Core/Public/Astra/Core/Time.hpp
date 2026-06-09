#pragma once

#include <Astra/Core/Types.hpp>

#include <chrono>

namespace Astra::Core {

struct TimePoint {
    u64 monotonic_ns = 0;
};

struct GameTime {
    u64 frame_index = 0;
    double fixed_delta_seconds = 1.0 / 60.0;
    double elapsed_seconds = 0.0;
    bool paused = false;
};

class MonotonicClock {
public:
    [[nodiscard]] TimePoint Now() const;
};

class FixedStepClock {
public:
    explicit FixedStepClock(double fixed_delta_seconds = 1.0 / 60.0);
    void Tick();
    void SetPaused(bool paused);
    [[nodiscard]] const GameTime& State() const;

private:
    GameTime time_;
};

} // namespace Astra::Core

