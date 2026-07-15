type UiAction = { id: string, [string]: unknown }
type UiEffect = { kind: string, [string]: unknown }

type UiControllerManifest = {
    schema: "astra.vn.ui_controller.v1",
    view: string,
    model_schema: string,
    snapshot: "none" | "session",
}

type UiControllerHandlers = {
    on_open: ((context: unknown, model: unknown) -> { UiEffect })?,
    on_action: ((context: unknown, model: unknown, action: UiAction) -> { UiEffect })?,
    on_close: ((context: unknown, model: unknown) -> { UiEffect })?,
}

type AstraUiApi = {
    ui: {
        controller: {
            register: (id: string, manifest: UiControllerManifest, handlers: UiControllerHandlers) -> (),
        },
        effect: {
            forward: (action: UiAction) -> UiEffect,
            open_modal: (view_id: string, model: unknown) -> UiEffect,
            close_modal: () -> UiEffect,
            focus: (semantic_id: string) -> UiEffect,
            set_state: (key: string, value: unknown) -> UiEffect,
            animate: (target_id: string, preset_id: string) -> UiEffect,
            trace: (kind: string, fields: { [string]: unknown }) -> UiEffect,
        },
    },
}

local astra: AstraUiApi = nil :: any
