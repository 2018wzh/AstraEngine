#include <Astra/Core/Time.h>

namespace astra {

SteadyTimePoint steady_now() {
    return std::chrono::steady_clock::now();
}

double seconds_since(SteadyTimePoint start, SteadyTimePoint end) {
    return std::chrono::duration<double>(end - start).count();
}

FrameTimer::FrameTimer(SteadyTimePoint start) : start_(start), last_tick_(start) {}

double FrameTimer::tick() {
    const SteadyTimePoint current = steady_now();
    const double delta = seconds_since(last_tick_, current);
    last_tick_ = current;
    return delta;
}

double FrameTimer::total_seconds() const {
    return seconds_since(start_);
}

SteadyTimePoint FrameTimer::last_tick_time() const {
    return last_tick_;
}

} // namespace astra
