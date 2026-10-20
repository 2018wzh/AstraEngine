use super::*;
use std::{cell::RefCell, rc::Rc};

/// Which editable field shows a failed save under itself instead of a notice.
#[derive(Clone, Copy)]
pub(super) enum FieldScope {
    Game,
    EmulatorDefault,
    Filter,
    GameRender,
    DefaultRender,
}

/// Shared handles for UI callbacks: run a controller action, then show the new
/// model or report the error.
pub(super) struct Wiring<C> {
    pub(super) controller: Rc<RefCell<C>>,
    pub(super) adapter: std::rc::Weak<SlintManagerAdapter>,
    pub(super) window: slint::Weak<astra_emu_manager_ui_slint::ManagerWindow>,
    pub(super) gamepad: Rc<RefCell<GameInputPump>>,
}

impl<C> Clone for Wiring<C> {
    fn clone(&self) -> Self {
        Self {
            controller: self.controller.clone(),
            adapter: self.adapter.clone(),
            window: self.window.clone(),
            gamepad: self.gamepad.clone(),
        }
    }
}

impl<C: ManagerController> Wiring<C> {
    pub(super) fn new(
        adapter: &Rc<SlintManagerAdapter>,
        controller: &Rc<RefCell<C>>,
        gamepad: &Rc<RefCell<GameInputPump>>,
    ) -> Self {
        Self {
            controller: controller.clone(),
            adapter: Rc::downgrade(adapter),
            window: adapter.window().as_weak(),
            gamepad: gamepad.clone(),
        }
    }

    pub(super) fn show(&self, result: Result<ManagerViewModel, String>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        match result {
            Ok(model) => apply_model(&self.adapter, &model),
            Err(error) => report_error(&window, &error),
        }
    }

    pub(super) fn run(&self, action: impl FnOnce(&mut C) -> Result<ManagerViewModel, String>) {
        let result = action(&mut self.controller.borrow_mut());
        self.show(result);
    }

    /// Like `run`, then hand the resulting pump mapping to the gamepad worker.
    pub(super) fn run_mapping(
        &self,
        action: impl FnOnce(&mut C) -> Result<ManagerViewModel, String>,
    ) {
        let result = {
            let mut controller = self.controller.borrow_mut();
            action(&mut controller).and_then(|model| {
                self.gamepad
                    .borrow_mut()
                    .set_mapping(controller.input_mapping())?;
                Ok(model)
            })
        };
        self.show(result);
    }

    /// Persisting a field failed: show the stored value again with the error under the field.
    pub(super) fn run_field(
        &self,
        scope: FieldScope,
        key: &str,
        action: impl FnOnce(&mut C) -> Result<ManagerViewModel, String>,
    ) {
        let result = action(&mut self.controller.borrow_mut());
        let Err(error) = result else {
            return self.show(result);
        };
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let model = self.controller.borrow().model();
        let mut model = match model {
            Ok(model) => model,
            Err(model_error) => return report_error(&window, &model_error),
        };
        let text = describe_error(&error).title;
        let fields = match scope {
            FieldScope::Game => &mut model.game_settings.fields,
            FieldScope::EmulatorDefault => &mut model.cores.default_fields,
            FieldScope::Filter => &mut model.filters.fields,
            FieldScope::GameRender => {
                model.game_settings.render.error = text;
                return apply_model(&self.adapter, &model);
            }
            FieldScope::DefaultRender => {
                model.cores.default_render.error = text;
                return apply_model(&self.adapter, &model);
            }
        };
        match fields.iter_mut().find(|field| field.key == key) {
            Some(field) => field.error = text,
            None => report_error(&window, &error),
        }
        apply_model(&self.adapter, &model);
    }
}
