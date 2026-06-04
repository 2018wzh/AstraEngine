#pragma once

#include <chrono>

namespace astra {

using SteadyTimePoint = std::chrono::steady_clock::time_point;
using Duration = std::chrono::duration<double>;

SteadyTimePoint steady_now();
double seconds_since(SteadyTimePoint start, SteadyTimePoint end = steady_now());

class FrameTimer {
  public:
    explicit FrameTimer(SteadyTimePoint start = steady_now());

    [[nodiscard]] double tick();
    [[nodiscard]] double total_seconds() const;
    [[nodiscard]] SteadyTimePoint last_tick_time() const;

  private:
    SteadyTimePoint start_;
    SteadyTimePoint last_tick_;
};

} // namespace astra
