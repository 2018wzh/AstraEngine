#include "tjsCommHead.h"
#ifndef KRKR2_ASTRA_HOSTED
#include "cocos2d.h"
#endif

#include "TVPScreen.h"
#include "Application.h"

int tTVPScreen::GetWidth() { return 2048; }
int tTVPScreen::GetHeight() {
#ifdef KRKR2_ASTRA_HOSTED
    // Hosted sessions render offscreen; the screen query only feeds window
    // placement defaults, so a fixed 16:9 ratio is sufficient.
    return 1152;
#else
    const cocos2d::Size &size =
        cocos2d::Director::getInstance()->getOpenGLView()->getFrameSize();
    int w = GetWidth();
    int h = w * (size.height / size.width);
    return w;
#endif
}

int tTVPScreen::GetDesktopLeft() { return 0; }
int tTVPScreen::GetDesktopTop() { return 0; }
int tTVPScreen::GetDesktopWidth() { return GetWidth(); }
int tTVPScreen::GetDesktopHeight() { return GetHeight(); }
