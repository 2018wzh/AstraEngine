// Hosted stand-in for the cocos2d math types (KRKR2_ASTRA_HOSTED only). The
// draw-device interface stores a raw 4x4 transform; only element storage, the
// row-major set() helper, and raw component access are consumed here.
#pragma once

namespace cocos2d {

class Mat4 {
public:
    float m[16];
    Mat4() : m{} {}

    void set(float m0, float m1, float m2, float m3,
             float m4, float m5, float m6, float m7,
             float m8, float m9, float m10, float m11,
             float m12, float m13, float m14, float m15) {
        m[0] = m0; m[1] = m1; m[2] = m2; m[3] = m3;
        m[4] = m4; m[5] = m5; m[6] = m6; m[7] = m7;
        m[8] = m8; m[9] = m9; m[10] = m10; m[11] = m11;
        m[12] = m12; m[13] = m13; m[14] = m14; m[15] = m15;
    }
};

} // namespace cocos2d
