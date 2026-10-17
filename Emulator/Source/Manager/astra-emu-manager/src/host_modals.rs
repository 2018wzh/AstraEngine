use super::*;
use crate::modal_service::{ModalPresentation, ModalUiUpdate};
use astra_emu_manager_core::model::ModalResponse;
use std::{cell::RefCell, rc::Rc};

pub(super) type ModalUiState = Rc<RefCell<Option<ModalPresentation>>>;

pub(super) fn install<C: ManagerController>(
    adapter: &Rc<SlintManagerAdapter>,
    controller: &Rc<RefCell<C>>,
) -> ModalUiState {
    let state: ModalUiState = Rc::new(RefCell::new(None));
    let selected = state.clone();
    let target = controller.clone();
    let weak = adapter.window().as_weak();
    adapter.window().on_modal_submit(move |button, value| {
        let Some(presentation) = selected.borrow().as_ref().cloned() else {
            return;
        };
        let Some(window) = weak.upgrade() else {
            return;
        };
        let Ok(button_index) = u32::try_from(button) else {
            window.set_global_diagnostic("ASTRA_EMU_FAMILY_MODAL_RESPONSE".into());
            return;
        };
        let text = if presentation.request.initial_text.is_some() {
            Some(value.as_str().into())
        } else {
            None
        };
        match target.borrow_mut().respond_modal(
            presentation.id,
            ModalResponse::Submitted { button_index, text },
        ) {
            Ok(()) => {
                selected.borrow_mut().take();
                window.set_modal_active(false);
            }
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let selected = state.clone();
    let target = controller.clone();
    let weak = adapter.window().as_weak();
    adapter.window().on_modal_cancel(move || {
        let Some(presentation) = selected.borrow_mut().take() else {
            return;
        };
        let result = target
            .borrow_mut()
            .respond_modal(presentation.id, ModalResponse::Cancelled);
        if let Some(window) = weak.upgrade() {
            window.set_modal_active(false);
            if let Err(error) = result {
                window.set_global_diagnostic(error.into());
            }
        }
    });
    state
}

pub(super) fn service<C: ManagerController>(
    window: &astra_emu_manager_ui_slint::ManagerWindow,
    controller: &Rc<RefCell<C>>,
    state: &ModalUiState,
) {
    loop {
        let update = controller.borrow_mut().take_modal_update();
        match update {
            Some(ModalUiUpdate::Show(presentation)) => {
                let request = &presentation.request;
                window.set_modal_caption(request.caption.as_str().into());
                window.set_modal_message(request.message.as_str().into());
                window.set_modal_has_input(request.initial_text.is_some());
                window.set_modal_text(match &request.initial_text {
                    Some(text) => text.as_str().into(),
                    None => slint::SharedString::default(),
                });
                let buttons: Vec<slint::SharedString> = request
                    .buttons
                    .iter()
                    .map(|label| label.as_str().into())
                    .collect();
                window.set_modal_buttons(slint::ModelRc::new(slint::VecModel::from(buttons)));
                state.replace(Some(presentation));
                window.set_modal_active(true);
            }
            Some(ModalUiUpdate::Dismiss(id)) => {
                if state
                    .borrow()
                    .as_ref()
                    .is_some_and(|presentation| presentation.id == id)
                {
                    state.borrow_mut().take();
                    window.set_modal_active(false);
                }
            }
            None => break,
        }
    }
}
