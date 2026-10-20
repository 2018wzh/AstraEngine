use super::*;
use astra_emu_manager_ui_slint::SnackbarViewModel;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SnackbarAction {
    Undo(String),
    Restart,
}

impl SnackbarAction {
    fn label(&self) -> &'static str {
        match self {
            Self::Undo(_) => "撤销",
            Self::Restart => "立即重启",
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct Snackbar {
    serial: u32,
    text: String,
    action: Option<SnackbarAction>,
}

impl Snackbar {
    pub(super) fn view(&self) -> SnackbarViewModel {
        SnackbarViewModel {
            serial: self.serial,
            text: self.text.clone(),
            action: self
                .action
                .as_ref()
                .map(SnackbarAction::label)
                .unwrap_or_default()
                .into(),
        }
    }

    pub(super) fn take_action(&mut self) -> Option<SnackbarAction> {
        self.action.take()
    }
}

impl AstraEmuManagerController {
    pub(super) fn show_snackbar(
        &mut self,
        text: impl Into<String>,
        action: Option<SnackbarAction>,
    ) {
        self.snackbar.serial = self.snackbar.serial.wrapping_add(1);
        self.snackbar.text = text.into();
        self.snackbar.action = action;
    }
}
