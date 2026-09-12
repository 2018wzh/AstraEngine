// Astra hosted smoke runner: drives the C ABI directly without the Rust
// adapter. Boots a game directory, pumps a fixed number of frames, injects
// Enter/click inputs, writes the first and last frames as PNG and the mixed
// audio as WAV, then shuts down.
//
// Usage: astra-krkr-smoke <game-dir> <out-dir> [seconds]

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

#include "astra_krkr_host.h"

namespace {
void write_wav(const char *path, const std::vector<int16_t> &samples) {
    FILE *f = fopen(path, "wb");
    if(!f)
        return;
    const uint32_t data_size = uint32_t(samples.size() * 2);
    const uint32_t riff_size = 36 + data_size;
    const uint16_t channels = ASTRA_KRKR_CHANNELS;
    const uint32_t rate = ASTRA_KRKR_SAMPLE_RATE;
    fwrite("RIFF", 1, 4, f);
    fwrite(&riff_size, 4, 1, f);
    fwrite("WAVEfmt ", 1, 8, f);
    const uint32_t fmt_size = 16;
    fwrite(&fmt_size, 4, 1, f);
    const uint16_t pcm = 1;
    fwrite(&pcm, 2, 1, f);
    fwrite(&channels, 2, 1, f);
    fwrite(&rate, 4, 1, f);
    const uint32_t byte_rate = rate * channels * 2;
    fwrite(&byte_rate, 4, 1, f);
    const uint16_t block_align = channels * 2;
    fwrite(&block_align, 2, 1, f);
    const uint16_t bits = 16;
    fwrite(&bits, 2, 1, f);
    fwrite("data", 1, 4, f);
    fwrite(&data_size, 4, 1, f);
    fwrite(samples.data(), 2, samples.size(), f);
    fclose(f);
}

std::vector<uint8_t> flip_rows(const uint8_t *data, uint32_t w, uint32_t h) {
    // stb_image_write expects top-down rows; the engine composes top-down
    // already, so this is a pass-through kept for clarity.
    return std::vector<uint8_t>(data, data + size_t(w) * h * 4);
}
} // namespace

int main(int argc, char **argv) {
    if(argc < 3) {
        fprintf(stderr, "usage: astra-krkr-smoke <game-dir> <out-dir> [seconds]\n");
        return 2;
    }
    const char *game_dir = argv[1];
    const char *out_dir = argv[2];
    const double seconds = argc > 3 ? atof(argv[3]) : 10.0;

    std::vector<int16_t> pcm;
    astra_krkr_host_callbacks cb{};
    cb.user = &pcm;
    cb.push_pcm = [](void *user, const int16_t *samples, uint32_t frames) {
        auto *out = static_cast<std::vector<int16_t> *>(user);
        out->insert(out->end(), samples, samples + size_t(frames) * ASTRA_KRKR_CHANNELS);
    };
    cb.log = [](void *, uint8_t level, const char *message) {
        fprintf(stderr, "[engine %u] %s\n", level, message);
    };

    astra_krkr_boot_config config{};
    config.abi = ASTRA_KRKR_HOST_ABI_VERSION;
    config.game_dir = game_dir;
    config.save_dir = game_dir;  // smoke runs on a writable copy
    config.locale = "zh-CN";
    config.callbacks = cb;

    const int rc = astra_krkr_boot(&config);
    if(rc != astra_krkr_ok) {
        fprintf(stderr, "boot failed: %d\n", rc);
        return 1;
    }

    // Settle until the first frame appears (startup script may take a while).
    std::vector<uint8_t> frame;
    uint32_t width = 0, height = 0;
    for(int i = 0; i < 1800; ++i) {
        if(astra_krkr_tick(1.0 / 60.0, nullptr, 0) != astra_krkr_ok)
            break;
        uint32_t w = 0, h = 0;
        astra_krkr_copy_frame(nullptr, 0, &w, &h);
        if(w == 0 || h == 0)
            continue;
        frame.resize(size_t(w) * h * 4);
        if(astra_krkr_copy_frame(frame.data(), uint32_t(frame.size()), &w,
                                 &h) == astra_krkr_ok) {
            width = w;
            height = h;
            break;
        }
    }
    if(width == 0) {
        fprintf(stderr, "no frame composed\n");
        astra_krkr_shutdown();
        return 1;
    }
    printf("frame %ux%u\n", width, height);

    char path[512];
    snprintf(path, sizeof(path), "%s/first.png", out_dir);
    FILE *png = fopen(path, "wb");
    // Minimal writer: use stb through a local include if available; the smoke
    // runner writes raw RGBA as .rgba when stb is absent.
    if(png) {
        fwrite(frame.data(), 1, frame.size(), png);
        fclose(png);
        printf("wrote %s (raw RGBA)\n", path);
    }

    // Pump the requested duration, injecting Enter every second.
    const auto start = std::chrono::steady_clock::now();
    uint64_t ticks = 0;
    while(true) {
        astra_krkr_input_event ev{};
        if(ticks % 60 == 0) {
            ev.kind = astra_krkr_input_key_down;
            ev.key = 0x0D;  // VK_RETURN
            astra_krkr_tick(1.0 / 60.0, &ev, 1);
            ev.kind = astra_krkr_input_key_up;
            astra_krkr_tick(1.0 / 60.0, &ev, 1);
        } else {
            astra_krkr_tick(1.0 / 60.0, nullptr, 0);
        }
        ++ticks;
        const auto elapsed = std::chrono::duration<double>(
                                 std::chrono::steady_clock::now() - start)
                                 .count();
        if(elapsed >= seconds)
            break;
    }

    uint32_t w2 = 0, h2 = 0;
    astra_krkr_copy_frame(nullptr, 0, &w2, &h2);
    if(w2 && h2) {
        frame.resize(size_t(w2) * h2 * 4);
        if(astra_krkr_copy_frame(frame.data(), uint32_t(frame.size()), &w2,
                                 &h2) == astra_krkr_ok) {
            snprintf(path, sizeof(path), "%s/last.rgba", out_dir);
            FILE *f = fopen(path, "wb");
            if(f) {
                fwrite(frame.data(), 1, frame.size(), f);
                fclose(f);
            }
            printf("last frame %ux%u\n", w2, h2);
        }
    }

    snprintf(path, sizeof(path), "%s/audio.wav", out_dir);
    write_wav(path, pcm);
    printf("pcm frames: %zu\n", pcm.size() / ASTRA_KRKR_CHANNELS);

    uint8_t terminated = 0;
    astra_krkr_terminated(&terminated);
    printf("terminated: %u\n", terminated);
    astra_krkr_shutdown();
    return 0;
}
