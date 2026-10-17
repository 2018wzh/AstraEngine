#pragma once
#include "NativeOwner.h"
#include <array>

namespace astra::krkr {
class NativeInput {
    std::array<bool, 256> held_{};
    std::array<bool, 5> buttons_{};
    float x_ = 0, y_ = 0;
    double wheel_ = 0;
    bool suspended_ = false;
    bool reset_clock_ = false;
    void key(unsigned int code, bool pressed, bool repeated = false);
public:
    void apply(const AstraKrkrEvent &event);
    void release();
    bool suspended() const noexcept { return suspended_; }
    bool take_reset_clock() noexcept;
};
}
