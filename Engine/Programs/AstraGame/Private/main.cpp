#include <Astra/AstraRuntime/AstraRuntimeSession.h>
#include <Astra/Bootstrap/RuntimeProviderRegistry.h>
#include <Astra/Core/Diagnostics.h>
#include <Astra/Core/Log.h>
#include <Astra/Core/Path.h>
#include <Astra/ModuleRuntime/ModuleManager.h>

#include <chrono>
#include <filesystem>
#include <iostream>
#include <string>
#include <thread>
#include <vector>

namespace {

struct CliOptions {
    std::filesystem::path project = ASTRA_SAMPLE_PROJECT_ROOT;
    std::vector<std::filesystem::path> plugin_roots{ASTRA_DEFAULT_PLUGIN_ROOT};
    bool headless = false;
    std::string route = "default";
};

struct BootstrapContext {
    astra::RuntimeProviderRegistry providers;
    astra::ExtensionRegistry extensions;
    astra::ModuleManager modules;

    BootstrapContext() : modules(extensions, &providers) {}
};

CliOptions parse_cli(int argc, char** argv) {
    CliOptions options;
    for (int i = 1; i < argc; ++i) {
        const std::string arg = argv[i];
        if (arg == "--project" && i + 1 < argc) {
            options.project = argv[++i];
        } else if (arg == "--plugin-root" && i + 1 < argc) {
            options.plugin_roots.push_back(argv[++i]);
        } else if (arg == "--headless") {
            options.headless = true;
        } else if (arg == "--route" && i + 1 < argc) {
            options.route = argv[++i];
        }
    }
    return options;
}

void log_diagnostics(const astra::DiagnosticSink& diagnostics) {
    astra::log::write_diagnostics(diagnostics);
}

bool initialize_bootstrap(BootstrapContext& context, const CliOptions& options,
                          astra::DiagnosticSink& diagnostics) {
    if (auto discovered = context.modules.discover(options.plugin_roots, diagnostics);
        !discovered) {
        return false;
    }
    if (auto loaded = context.modules.load_discovered(diagnostics); !loaded) {
        return false;
    }
    return !diagnostics.has_errors();
}

int run_headless(const CliOptions& options) {
    astra::DiagnosticSink diagnostics;
    BootstrapContext bootstrap;
    if (!initialize_bootstrap(bootstrap, options, diagnostics)) {
        log_diagnostics(diagnostics);
        return 1;
    }

    astra::AstraRuntimeSession session(bootstrap.providers, bootstrap.extensions);
    if (auto loaded = session.load_project(options.project, diagnostics); !loaded) {
        log_diagnostics(diagnostics);
        return 1;
    }
    if (auto started = session.start(diagnostics); !started) {
        log_diagnostics(diagnostics);
        return 1;
    }

    for (int step = 0; step < 32; ++step) {
        const auto snapshot = session.render_snapshot();
        if (!snapshot.choices.empty()) {
            const std::size_t choice =
                options.route == "second" && snapshot.choices.size() > 1 ? 1 : 0;
            if (auto chosen = session.choose(choice, diagnostics); !chosen) {
                break;
            }
        } else if (auto advanced = session.advance(diagnostics); !advanced) {
            break;
        }
    }

    for (const std::string& command : session.command_log()) {
        std::cout << command << '\n';
    }
    session.shutdown(diagnostics);
    bootstrap.modules.unload_all(diagnostics);
    log_diagnostics(diagnostics);
    return diagnostics.has_errors() ? 1 : 0;
}

void play_audio_requests(astra::AstraRuntimeSession& session, astra::IAudioRuntime& audio) {
    for (const astra::RuntimeAudioRequest& request : session.consume_audio_requests()) {
        auto source = session.resolve_asset_source(request.asset_id);
        if (source) {
            audio.play_sound(*source);
        }
    }
}

int run_visual(const CliOptions& options) {
    astra::DiagnosticSink diagnostics;
    BootstrapContext bootstrap;
    if (!initialize_bootstrap(bootstrap, options, diagnostics)) {
        log_diagnostics(diagnostics);
        return 1;
    }

    {
        auto platform_provider = bootstrap.providers.platform_provider();
        auto renderer_provider = bootstrap.providers.renderer_provider();
        auto audio_provider = bootstrap.providers.audio_provider();
        if (!platform_provider || !renderer_provider || !audio_provider) {
            if (!platform_provider) {
                diagnostics.error(platform_provider.error().code,
                                  platform_provider.error().message);
            }
            if (!renderer_provider) {
                diagnostics.error(renderer_provider.error().code,
                                  renderer_provider.error().message);
            }
            if (!audio_provider) {
                diagnostics.error(audio_provider.error().code, audio_provider.error().message);
            }
            log_diagnostics(diagnostics);
            return 1;
        }

        auto platform = (*platform_provider)->create_platform(diagnostics);
        if (platform == nullptr || !platform->is_initialized()) {
            log_diagnostics(diagnostics);
            return 1;
        }
        auto window =
            (*platform_provider)
                ->create_window(*platform, {1280, 720, "AstraGame - MinimalVN"}, diagnostics);
        if (window == nullptr || !window->is_open()) {
            log_diagnostics(diagnostics);
            return 1;
        }
        auto renderer = (*renderer_provider)->create_renderer(*window, diagnostics);
        if (renderer == nullptr || !renderer->available()) {
            log_diagnostics(diagnostics);
            return 1;
        }
        auto audio = (*audio_provider)->create_audio(diagnostics);
        if (audio == nullptr) {
            log_diagnostics(diagnostics);
            return 1;
        }

        astra::AstraRuntimeSession session(bootstrap.providers, bootstrap.extensions);
        if (auto loaded = session.load_project(options.project, diagnostics); !loaded) {
            log_diagnostics(diagnostics);
            return 1;
        }
        if (auto started = session.start(diagnostics); !started) {
            log_diagnostics(diagnostics);
            return 1;
        }

        play_audio_requests(session, *audio);

        while (window->is_open()) {
            while (auto event = window->poll_event()) {
                if (event->type == astra::PlatformEventType::Quit) {
                    break;
                }
                if (event->type == astra::PlatformEventType::Advance) {
                    (void)session.advance(diagnostics);
                    play_audio_requests(session, *audio);
                } else if (event->type == astra::PlatformEventType::Choice1) {
                    (void)session.choose(0, diagnostics);
                    play_audio_requests(session, *audio);
                } else if (event->type == astra::PlatformEventType::Choice2) {
                    (void)session.choose(1, diagnostics);
                    play_audio_requests(session, *audio);
                }
            }

            session.tick();
            const auto snapshot = session.render_snapshot();
            std::string title = "AstraGame - " + snapshot.speaker + ": " + snapshot.dialogue;
            window->set_title(title.c_str());
            renderer->render(snapshot);
            std::this_thread::sleep_for(std::chrono::milliseconds(16));
        }

        session.shutdown(diagnostics);
    }
    bootstrap.modules.unload_all(diagnostics);
    log_diagnostics(diagnostics);
    return diagnostics.has_errors() ? 1 : 0;
}

} // namespace

int main(int argc, char** argv) {
    const auto options = parse_cli(argc, argv);
    astra::log::InitializeOptions log_options;
    log_options.log_directory = options.project / "Saved" / "Logs";
    log_options.file_stem = "AstraGame";
    astra::log::initialize(log_options);
    const int result = options.headless ? run_headless(options) : run_visual(options);
    astra::log::shutdown();
    return result;
}
