#include <Astra/Media/Media.hpp>

#include "MediaInternal.hpp"

#include <algorithm>
#include <utility>

namespace Astra::Media {

namespace {

Astra::Core::Diagnostic MakeDiagnostic(std::string code, Astra::Core::DiagnosticSeverity severity, std::string message) {
    Astra::Core::Diagnostic diagnostic;
    diagnostic.code = std::move(code);
    diagnostic.category = "media.backend";
    diagnostic.severity = severity;
    diagnostic.message = std::move(message);
    return diagnostic;
}

MediaProviderDescriptor FindProvider(std::string_view provider_id) {
    for (const auto& provider : ProductionMediaProviders()) {
        if (provider.provider_id == provider_id) {
            return provider;
        }
    }
    return {};
}

class HeadlessRenderer2DProvider final : public IRenderer2DProvider {
public:
    MediaProviderDescriptor Describe() const override { return FindProvider("astra.renderer2d.sdl"); }

    Astra::Core::Result<void> BeginFrame(const RenderFrameDesc& desc, Astra::Core::DiagnosticSink&) override {
        frame_ = desc;
        graph_ = {};
        imported_textures_.clear();
        return Astra::Core::Result<void>::Success();
    }

    Astra::Core::Result<TextureToken> ImportTexture(const DecodedCpuBuffer& buffer, Astra::Core::DiagnosticSink& diagnostics) override {
        if (buffer.width == 0 || buffer.height == 0 || buffer.pixels.empty()) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_RENDERER2D_TEXTURE_EMPTY", Astra::Core::DiagnosticSeverity::Blocking, "Decoded texture buffer is empty."));
            return Astra::Core::Result<TextureToken>::Failure(Astra::Core::ErrorCode::InvalidArgument, "empty texture");
        }
        imported_textures_.push_back(buffer);
        return Astra::Core::Result<TextureToken>::Success(TextureToken{static_cast<Astra::Core::u64>(imported_textures_.size())});
    }

    Astra::Core::Result<TextureToken> ImportSurface(MediaSurfaceToken token, Astra::Core::DiagnosticSink& diagnostics) override {
        if (token.Empty()) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_RENDERER2D_SURFACE_EMPTY", Astra::Core::DiagnosticSeverity::Blocking, "Media surface token is empty."));
            return Astra::Core::Result<TextureToken>::Failure(Astra::Core::ErrorCode::InvalidArgument, "empty surface");
        }
        return Astra::Core::Result<TextureToken>::Success(TextureToken{token.id});
    }

    Astra::Core::Result<void> Execute(const RenderGraph& graph, Astra::Core::DiagnosticSink& diagnostics) override {
        graph_ = graph;
        std::ranges::sort(graph_.draws, [](const RenderDraw& left, const RenderDraw& right) {
            if (left.layer == right.layer) {
                if (left.order == right.order) {
                    return left.draw_id < right.draw_id;
                }
                return left.order < right.order;
            }
            return left.layer < right.layer;
        });
        for (const auto& draw : graph_.draws) {
            if (std::ranges::find(graph_.layers, draw.layer) == graph_.layers.end()) {
                diagnostics.Emit(MakeDiagnostic("ASTRA_RENDERER2D_LAYER_UNKNOWN", Astra::Core::DiagnosticSeverity::Blocking, "Render draw references an unknown layer."));
            }
        }
        return diagnostics.HasBlocking() ? Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidFormat, "renderer diagnostics")
                                         : Astra::Core::Result<void>::Success();
    }

    Astra::Core::Result<FrameCapture> Capture(Astra::Core::DiagnosticSink&) override {
        FrameCapture capture;
        capture.frame_index = frame_.frame_index == 0 ? graph_.frame_index : frame_.frame_index;
        capture.commands = ToJson(graph_);
        capture.commands["frame_desc"] = {{"width", frame_.width}, {"height", frame_.height}, {"color_space", frame_.color_space}};
        capture.commands["imported_texture_count"] = imported_textures_.size();
        capture.render_hash = Private::StableHash(capture.commands.at("draws").dump() + std::to_string(imported_textures_.size()));
        capture.text_hash = Private::StableHash(capture.commands.at("text_requests").dump());
        capture.audio_hash = Private::StableHash(capture.commands.at("audio_commands").dump());
        capture.filter_hash = Private::StableHash(capture.commands.at("filter_applications").dump());
        return Astra::Core::Result<FrameCapture>::Success(std::move(capture));
    }

    Astra::Core::Result<void> Present(PresentRequest request, Astra::Core::DiagnosticSink& diagnostics) override {
        if (!request.allow_headless && request.target_id.empty()) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_RENDERER2D_PRESENT_TARGET_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Present target is missing."));
            return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidArgument, "present target missing");
        }
        return Astra::Core::Result<void>::Success();
    }

private:
    RenderFrameDesc frame_;
    RenderGraph graph_;
    std::vector<DecodedCpuBuffer> imported_textures_;
};

class FoundationTextLayoutProvider final : public ITextLayoutProvider {
public:
    MediaProviderDescriptor Describe() const override { return FindProvider("astra.text_layout.freetype_harfbuzz"); }

    Astra::Core::Result<GlyphRun> Shape(TextLayoutRequest request, Astra::Core::DiagnosticSink& diagnostics) override {
        if (request.text.empty()) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_TEXT_LAYOUT_EMPTY", Astra::Core::DiagnosticSeverity::Blocking, "Text layout request is empty."));
            return Astra::Core::Result<GlyphRun>::Failure(Astra::Core::ErrorCode::InvalidArgument, "empty text");
        }
        GlyphRun run;
        run.request_id = std::move(request.request_id);
        run.locale = std::move(request.locale);
        run.glyph_count = static_cast<Astra::Core::u32>(request.text.size());
        run.run_hash = Private::StableHash(request.text + "|" + run.locale + "|" + request.style.dump());
        runs_.push_back(run);
        return Astra::Core::Result<GlyphRun>::Success(std::move(run));
    }

    Astra::Core::Result<GlyphAtlasToken> PrepareAtlas(const GlyphRun& run, Astra::Core::DiagnosticSink& diagnostics) override {
        if (run.glyph_count == 0) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_TEXT_LAYOUT_GLYPHS_EMPTY", Astra::Core::DiagnosticSeverity::Blocking, "Glyph run is empty."));
            return Astra::Core::Result<GlyphAtlasToken>::Failure(Astra::Core::ErrorCode::InvalidArgument, "empty glyph run");
        }
        return Astra::Core::Result<GlyphAtlasToken>::Success(GlyphAtlasToken{static_cast<Astra::Core::u64>(runs_.size())});
    }

    Astra::Core::Result<TextLayoutCapture> Capture(Astra::Core::DiagnosticSink&) override {
        TextLayoutCapture capture;
        capture.glyph_runs = runs_;
        nlohmann::json json = nlohmann::json::array();
        for (const auto& run : runs_) {
            json.push_back(ToJson(run));
        }
        capture.glyph_hash = Private::StableHash(json.dump());
        return Astra::Core::Result<TextLayoutCapture>::Success(std::move(capture));
    }

private:
    std::vector<GlyphRun> runs_;
};

class FoundationAudioProvider final : public IAudioProvider {
public:
    explicit FoundationAudioProvider(bool silent_backend) : silent_backend_(silent_backend) {}

    MediaProviderDescriptor Describe() const override { return FindProvider("astra.audio.miniaudio"); }

    Astra::Core::Result<AudioStreamToken> CreateStream(const AudioCommand& command, Astra::Core::DiagnosticSink& diagnostics) override {
        if (command.asset.path.empty()) {
            diagnostics.Emit(MakeDiagnostic("ASTRA_AUDIO_STREAM_ASSET_MISSING", Astra::Core::DiagnosticSeverity::Blocking, "Audio stream requires an asset."));
            return Astra::Core::Result<AudioStreamToken>::Failure(Astra::Core::ErrorCode::InvalidArgument, "audio asset missing");
        }
        active_.push_back(command);
        return Astra::Core::Result<AudioStreamToken>::Success(AudioStreamToken{static_cast<Astra::Core::u64>(active_.size())});
    }

    Astra::Core::Result<void> Submit(const std::vector<AudioCommand>& commands, Astra::Core::DiagnosticSink&) override {
        for (const auto& command : commands) {
            if (command.kind == "stop") {
                std::erase_if(active_, [&](const AudioCommand& active) { return active.bus == command.bus; });
            } else if (command.kind == "pause" || command.kind == "resume") {
                active_.push_back(command);
            } else {
                active_.push_back(command);
            }
            bus_volumes_[command.bus.empty() ? "sfx" : command.bus] = command.volume;
        }
        return Astra::Core::Result<void>::Success();
    }

    Astra::Core::Result<AudioStateCapture> Capture(Astra::Core::DiagnosticSink&) override {
        AudioStateCapture capture;
        capture.bus_volumes = bus_volumes_;
        capture.active_commands = active_;
        capture.silent_backend = silent_backend_;
        nlohmann::json json = nlohmann::json::array();
        for (const auto& command : active_) {
            json.push_back({{"command_id", command.command_id}, {"kind", command.kind}, {"asset", command.asset.ToString()}, {"bus", command.bus}, {"volume", command.volume}, {"loop", command.loop}});
        }
        capture.state_hash = Private::StableHash(json.dump());
        return Astra::Core::Result<AudioStateCapture>::Success(std::move(capture));
    }

private:
    bool silent_backend_ = true;
    std::map<std::string, double> bus_volumes_{{"voice", 1.0}, {"music", 1.0}, {"sfx", 1.0}, {"ui", 1.0}, {"ambient", 1.0}};
    std::vector<AudioCommand> active_;
};

} // namespace

std::unique_ptr<IRenderer2DProvider> CreateHeadlessRenderer2DProvider() {
    return std::make_unique<HeadlessRenderer2DProvider>();
}

std::unique_ptr<ITextLayoutProvider> CreateFoundationTextLayoutProvider() {
    return std::make_unique<FoundationTextLayoutProvider>();
}

std::unique_ptr<IAudioProvider> CreateFoundationAudioProvider(bool silent_backend) {
    return std::make_unique<FoundationAudioProvider>(silent_backend);
}

} // namespace Astra::Media
