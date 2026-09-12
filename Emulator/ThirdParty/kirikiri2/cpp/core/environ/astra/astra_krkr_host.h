// Astra hosted ABI between the AstraEMU Kirikiri family plugin (Rust) and
// this vendored Kirikiri core (C++). One session per process.
//
// Threading contract:
// - boot/tick/copy_frame/terminated/shutdown are called from one session
//   thread.
// - push_pcm is called from the engine audio worker thread.
// - log may be called from any engine thread.

#pragma once

#include <stdint.h>

/* Windows DLL export/import: the vendored static archives join the DLL, so
 * the automatic all-symbol export cannot see these; declare them explicitly. */
#if defined(_WIN32) && defined(ASTRA_KRKR_BUILDING)
#define ASTRA_KRKR_API __declspec(dllexport)
#elif defined(_WIN32)
#define ASTRA_KRKR_API __declspec(dllimport)
#else
#define ASTRA_KRKR_API
#endif

#ifdef __cplusplus
extern "C" {
#endif

#define ASTRA_KRKR_HOST_ABI_VERSION 1
#define ASTRA_KRKR_SAMPLE_RATE 48000
#define ASTRA_KRKR_CHANNELS 2

enum astra_krkr_status {
    astra_krkr_ok = 0,
    astra_krkr_err_state = 1,
    astra_krkr_err_boot = 2,
    astra_krkr_err_arg = 3,
    astra_krkr_err_terminated = 4,
};

enum astra_krkr_input_kind {
    astra_krkr_input_key_down = 1,
    astra_krkr_input_key_up = 2,
    astra_krkr_input_text = 3,
    astra_krkr_input_mouse_move = 4,
    astra_krkr_input_mouse_down = 5,
    astra_krkr_input_mouse_up = 6,
    astra_krkr_input_wheel = 7,
};

typedef struct astra_krkr_input_event {
    uint8_t kind;
    uint8_t button;  /* 0=left 1=right 2=middle */
    uint8_t shift;
    uint8_t control;
    uint16_t key;    /* Windows VK code for key events */
    uint32_t utf32;  /* UTF-32 codepoint for text events */
    int32_t x;       /* primary-layer pixel coordinates */
    int32_t y;
    int32_t wheel;   /* detents; positive scrolls away from the user */
} astra_krkr_input_event;

typedef struct astra_krkr_host_callbacks {
    void *user;
    /* Interleaved stereo int16 PCM at ASTRA_KRKR_SAMPLE_RATE. */
    void (*push_pcm)(void *user, const int16_t *samples, uint32_t frame_count);
    /* Bounded diagnostics; never game text or filesystem paths. */
    void (*log)(void *user, uint8_t level, const char *message);
} astra_krkr_host_callbacks;

typedef struct astra_krkr_boot_config {
    uint32_t abi;
    const char *game_dir;  /* UTF-8 */
    const char *save_dir;  /* UTF-8, writable engine state directory */
    const char *locale;    /* BCP-47 style, may be NULL */
    uint32_t initial_width;  /* 0 = engine default */
    uint32_t initial_height;
    astra_krkr_host_callbacks callbacks;
} astra_krkr_boot_config;

ASTRA_KRKR_API uint32_t astra_krkr_abi_version(void);

/* Boots the engine against game_dir. The engine composes its first frame
 * asynchronously through the startup script; copy_frame reports
 * astra_krkr_err_state until then. */
ASTRA_KRKR_API int32_t astra_krkr_boot(const astra_krkr_boot_config *config);

/* Pumps one frame worth of engine processing after delivering input. */
ASTRA_KRKR_API int32_t astra_krkr_tick(double elapsed_seconds,
                       const astra_krkr_input_event *events, uint32_t event_count);

/* Reports the current frame size through width/height on every call. When
 * capacity is too small the function returns astra_krkr_err_arg after
 * filling the size, without touching dst. */
ASTRA_KRKR_API int32_t astra_krkr_copy_frame(uint8_t *dst, uint32_t capacity, uint32_t *width,
                              uint32_t *height);

ASTRA_KRKR_API int32_t astra_krkr_audio_format(uint32_t *sample_rate, uint16_t *channels);

ASTRA_KRKR_API int32_t astra_krkr_terminated(uint8_t *terminated);

/* Joins the audio worker and releases engine state. */
ASTRA_KRKR_API int32_t astra_krkr_shutdown(void);

/* Exposed for the vendored WaveMixer hosted renderer. */
void astra_krkr_hosted_push_pcm(const int16_t *samples, uint32_t frame_count);
void astra_krkr_hosted_log(uint8_t level, const char *message);

#ifdef __cplusplus
}
#endif
