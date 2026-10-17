#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <winsock2.h>
#include <windows.h>
#include "NativeInput.h"
#include "environ/cocos2d/MainScene.h"
#include "environ/Application.h"
#include "visual/impl/WindowImpl.h"
#include <cmath>
#include <limits>
#include <stdexcept>
#include <string>
#include <utility>

namespace astra::krkr {
static unsigned int key_code(std::uint32_t code) {
    if(code >= 1 && code <= 26) return 'A' + code - 1;
    if(code >= 27 && code <= 36) return '0' + code - 27;
    if(code >= 58 && code <= 69) return VK_F1 + code - 58;
    if(code >= 70 && code <= 79) return VK_NUMPAD0 + code - 70;
    switch(code) {
    case 0: return 0;
    case 37: return VK_ESCAPE; case 38: case 80: return VK_RETURN;
    case 39: return VK_TAB; case 40: return VK_BACK; case 41: return VK_SPACE;
    case 42: return VK_INSERT; case 43: return VK_DELETE; case 44: return VK_HOME;
    case 45: return VK_END; case 46: return VK_PRIOR; case 47: return VK_NEXT;
    case 48: return VK_LEFT; case 49: return VK_RIGHT; case 50: return VK_UP; case 51: return VK_DOWN;
    case 52: return VK_LCONTROL; case 53: return VK_RCONTROL;
    case 54: return VK_LSHIFT; case 55: return VK_RSHIFT;
    case 56: return VK_LMENU; case 57: return VK_RMENU;
    case 81: return VK_ADD; case 82: return VK_SUBTRACT; case 83: return VK_MULTIPLY; case 84: return VK_DIVIDE;
    case 85: return VK_OEM_MINUS; case 86: return VK_OEM_PLUS;
    case 87: return VK_OEM_COMMA; case 88: return VK_OEM_PERIOD; case 89: return VK_OEM_2;
    case 90: return VK_OEM_1; case 91: return VK_OEM_7;
    case 92: return VK_OEM_4; case 93: return VK_OEM_6; case 94: return VK_OEM_5; case 95: return VK_OEM_3;
    default: throw std::runtime_error("ASTRA_KRKR_NATIVE_KEY_CODE");
    }
}
static void validate_position(float value) {
    if(!std::isfinite(value) || value < std::numeric_limits<int>::min() ||
       static_cast<double>(value) > std::numeric_limits<int>::max())
        throw std::runtime_error("ASTRA_KRKR_NATIVE_POINTER_POSITION");
}
void NativeInput::key(unsigned int code, bool pressed, bool repeated) {
    if(!code) return;
    if(held_[code] != pressed || repeated) {
        held_[code] = pressed;
        TVPDispatchWindowKey(code, pressed);
    }
}
void NativeInput::release() {
    for(unsigned int code = 0; code < held_.size(); ++code)
        if(held_[code]) key(code, false);
    for(unsigned int button = 0; button < buttons_.size(); ++button) {
        if(buttons_[button]) {
            TVPDispatchWindowPointerButton(button, false, x_, y_);
            buttons_[button] = false;
        }
    }
    wheel_ = 0;
}
bool NativeInput::take_reset_clock() noexcept { return std::exchange(reset_clock_, false); }
void NativeInput::apply(const AstraKrkrEvent &event) {
    switch(event.kind) {
    case 1: {
        if(event.key_state > 2 || event.modifiers > 15)
            throw std::runtime_error("ASTRA_KRKR_NATIVE_KEY_STATE");
        const auto code = key_code(event.key_code);
        // Original modifier polling uses aggregate VK state. Keep it in sync
        // with the explicit Host snapshot, including focus regained mid-chord.
        key(VK_SHIFT, (event.modifiers & 1) != 0);
        key(VK_CONTROL, (event.modifiers & 2) != 0);
        key(VK_MENU, (event.modifiers & 4) != 0);
        key(VK_LWIN, (event.modifiers & 8) != 0);
        key(code, event.key_state != 1, event.key_state == 2);
        break;
    }
    case 2:
        validate_position(event.x); validate_position(event.y);
        x_ = event.x; y_ = event.y;
        TVPDispatchWindowPointerMove(x_, y_); break;
    case 3:
        if(event.pointer_button >= buttons_.size() || event.key_state > 2)
            throw std::runtime_error("ASTRA_KRKR_NATIVE_POINTER_BUTTON");
        buttons_[event.pointer_button] = event.key_state != 1;
        TVPDispatchWindowPointerButton(event.pointer_button, event.key_state != 1, x_, y_); break;
    case 4: {
        if(!std::isfinite(event.y) || event.x != 0)
            throw std::runtime_error("ASTRA_KRKR_NATIVE_WHEEL");
        const double delta = wheel_ + static_cast<double>(event.y) * 120;
        if(delta < std::numeric_limits<int>::min() || delta > std::numeric_limits<int>::max())
            throw std::runtime_error("ASTRA_KRKR_NATIVE_WHEEL_RANGE");
        const auto integral = static_cast<int>(delta);
        wheel_ = delta - integral;
        if(integral) TVPDispatchWindowWheel(integral, x_, y_);
        break;
    }
    case 5:
        if(event.text_bytes > 32768 || (event.text_bytes && !event.text_utf8))
            throw std::runtime_error("ASTRA_KRKR_NATIVE_TEXT_BOUND");
        if(event.text_bytes) TVPMainScene::onTextInput(std::string(
            reinterpret_cast<const char *>(event.text_utf8), event.text_bytes));
        break;
    case 6:
        if(event.value > 1) throw std::runtime_error("ASTRA_KRKR_NATIVE_FOCUS");
        if(event.value) Application->OnActivate();
        else { release(); Application->OnDeactivate(); }
        break;
    case 7:
        if(!event.width || !event.height) throw std::runtime_error("ASTRA_KRKR_NATIVE_WINDOW_SIZE");
        // Host output resize does not mutate the original logical game canvas.
        break;
    case 8:
        if(event.value > 1) throw std::runtime_error("ASTRA_KRKR_NATIVE_VISIBILITY");
        if(!event.value) release();
        break;
    case 9: {
        if(event.value > 1) throw std::runtime_error("ASTRA_KRKR_NATIVE_SUSPEND");
        const bool suspended = event.value != 0;
        if(suspended_ && !suspended) reset_clock_ = true;
        suspended_ = suspended;
        if(suspended_) release();
        break;
    }
    case 10:
        if(auto window = TVPGetCurrentWindowNative()) window->GetForm()->Close();
        break;
    default: throw std::runtime_error("ASTRA_KRKR_NATIVE_EVENT_KIND");
    }
}
}
