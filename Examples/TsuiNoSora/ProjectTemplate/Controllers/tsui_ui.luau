local controllers = {
    { "tsui.message.classic", "ui.tsui.classic.message", "astra.vn.ui_model.message.v1" },
    { "tsui.choice.classic", "ui.tsui.classic.choice", "astra.vn.ui_model.choice.v1" },
    { "tsui.system.title.classic", "ui.tsui.classic.title", "astra.vn.ui_model.title.v1" },
    { "tsui.system.save.classic", "ui.tsui.classic.save", "astra.vn.ui_model.save.v1" },
    { "tsui.system.load.classic", "ui.tsui.classic.load", "astra.vn.ui_model.load.v1" },
    { "tsui.message.modern", "ui.tsui.modern.message", "astra.vn.ui_model.message.v1" },
    { "tsui.choice.modern", "ui.tsui.modern.choice", "astra.vn.ui_model.choice.v1" },
    { "tsui.system.title.modern", "ui.tsui.modern.title", "astra.vn.ui_model.title.v1" },
    { "tsui.system.quick_panel.modern", "ui.tsui.modern.quick_panel", "astra.vn.ui_model.quick_panel.v1" },
    { "tsui.system.save.modern", "ui.tsui.modern.save", "astra.vn.ui_model.save.v1" },
    { "tsui.system.load.modern", "ui.tsui.modern.load", "astra.vn.ui_model.load.v1" },
    { "tsui.system.config.modern", "ui.tsui.modern.config", "astra.vn.ui_model.config.v1" },
    { "tsui.system.backlog.modern", "ui.tsui.modern.backlog", "astra.vn.ui_model.backlog.v1" },
}

for _, definition in controllers do
    local on_open = nil
    local on_action = function(_context, _model, action)
        return { astra.ui.effect.forward(action) }
    end
    if definition[1] == "tsui.system.quick_panel.modern" then
        on_open = function(_context, _model)
            return { astra.ui.effect.focus("root/commands/skip_all") }
        end
    end
    if definition[1] == "tsui.system.title.modern" then
        on_open = function(_context, _model)
            return { astra.ui.effect.focus("root/menu/continue") }
        end
    end
    if definition[1] == "tsui.system.save.modern" then
        on_open = function(_context, _model)
            return { astra.ui.effect.focus("root/content/slots/slot/slot.01/write") }
        end
    end
    if definition[1] == "tsui.system.load.modern" then
        on_open = function(_context, _model)
            return { astra.ui.effect.focus("root/content/slots/slot/slot.01/read") }
        end
    end
    if definition[1] == "tsui.system.config.modern" then
        on_open = function(_context, _model)
            return { astra.ui.effect.focus("root/content/master") }
        end
    end
    if definition[1] == "tsui.system.backlog.modern" then
        on_open = function(_context, _model)
            return { astra.ui.effect.focus("root/content/back") }
        end
    end
    if definition[1] == "tsui.choice.classic" or definition[1] == "tsui.choice.modern" then
        on_open = function(_context, model)
            for _, option_value in model.map.options.list do
                local option = option_value.map
                if option.enabled.bool then
                    return {
                        astra.ui.effect.focus(
                            "root/choices/options/option/" .. option.option_id.string
                        ),
                    }
                end
            end
            return {}
        end
    end
    if definition[1] == "tsui.system.save.modern" then
        on_action = function(_context, model, action)
            if action.request_save ~= nil then
                for _, slot_value in model.map.slots.list do
                    local slot = slot_value.map
                    if slot.slot_id.string == action.request_save.slot_id and slot.occupied.bool then
                        return {
                            astra.ui.effect.open_modal("ui.tsui.modern.save_confirm", {
                                slot_id = slot.slot_id.string,
                            }),
                        }
                    end
                end
            end
            return { astra.ui.effect.forward(action) }
        end
    end
    astra.ui.controller.register(definition[1], {
        schema = "astra.vn.ui_controller.v1",
        view = definition[2],
        model_schema = definition[3],
        snapshot = "session",
    }, { on_open = on_open, on_action = on_action })
end
