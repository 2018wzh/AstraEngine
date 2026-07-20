local controllers = {
    { "message.classic", "ui.message.classic", "astra.vn.ui_model.message.v2" },
    { "choice.classic", "ui.choice.classic", "astra.vn.ui_model.choice.v1" },
    { "system.standard", "ui.system.title", "astra.vn.ui_model.title.v1" },
    { "system.save", "ui.system.save", "astra.vn.ui_model.save.v1" },
    { "system.load", "ui.system.load", "astra.vn.ui_model.load.v1" },
    { "system.config", "ui.system.config", "astra.vn.ui_model.config.v1" },
    { "system.backlog", "ui.system.backlog", "astra.vn.ui_model.backlog.v1" },
    { "system.gallery", "ui.system.gallery", "astra.vn.ui_model.gallery.v1" },
    { "system.replay", "ui.system.replay", "astra.vn.ui_model.replay.v1" },
    { "system.voice", "ui.system.voice_replay", "astra.vn.ui_model.voice_replay.v1" },
    { "system.route", "ui.system.route_chart", "astra.vn.ui_model.route_chart.v1" },
    { "system.localization", "ui.system.localization", "astra.vn.ui_model.localization_preview.v1" },
    { "message.modern", "ui.message.modern", "astra.vn.ui_model.message.v2" },
    { "choice.modern", "ui.choice.modern", "astra.vn.ui_model.choice.v1" },
    { "system.modern", "ui.system.title", "astra.vn.ui_model.title.v1" },
    { "system.save.modern", "ui.system.save", "astra.vn.ui_model.save.v1" },
    { "system.load.modern", "ui.system.load", "astra.vn.ui_model.load.v1" },
    { "system.config.modern", "ui.system.config", "astra.vn.ui_model.config.v1" },
    { "system.backlog.modern", "ui.system.backlog", "astra.vn.ui_model.backlog.v1" },
    { "system.gallery.modern", "ui.system.gallery", "astra.vn.ui_model.gallery.v1" },
    { "system.replay.modern", "ui.system.replay", "astra.vn.ui_model.replay.v1" },
    { "system.voice.modern", "ui.system.voice_replay", "astra.vn.ui_model.voice_replay.v1" },
    { "system.route.modern", "ui.system.route_chart", "astra.vn.ui_model.route_chart.v1" },
    { "system.localization.modern", "ui.system.localization", "astra.vn.ui_model.localization_preview.v1" },
}

for _, definition in controllers do
    astra.ui.controller.register(definition[1], {
        schema = "astra.vn.ui_controller.v1",
        view = definition[2],
        model_schema = definition[3],
        snapshot = "none",
    }, {
        on_action = function(_context, _model, action)
            return { astra.ui.effect.forward(action) }
        end,
    })
end
