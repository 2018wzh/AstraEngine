#![allow(non_local_definitions)]

use abi_stable::{
    sabi_trait,
    std_types::{RBox, ROption, RString, RVec},
    StableAbi,
};

use super::descriptor::{FamilyError, FamilyResult, FfiFamilyResult};

pub const MAX_MODAL_TEXT_BYTES: usize = 65_536;
pub const MAX_MODAL_BUTTONS: usize = 16;
pub const MAX_MODAL_BUTTON_BYTES: usize = 256;

/// Original button order and labels; no UI type or native handle crosses the ABI.
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct ModalRequest {
    pub caption: RString,
    pub message: RString,
    pub buttons: RVec<RString>,
    /// None is a message box. Some is a text input, including an empty initial value.
    pub initial_text: ROption<RString>,
}

fn valid_text(text: &str, maximum: usize) -> bool {
    text.len() <= maximum && !text.contains('\0')
}

impl ModalRequest {
    pub fn validate(&self) -> FamilyResult<()> {
        if !valid_text(&self.caption, MAX_MODAL_TEXT_BYTES)
            || !valid_text(&self.message, MAX_MODAL_TEXT_BYTES)
            || self.buttons.is_empty()
            || self.buttons.len() > MAX_MODAL_BUTTONS
            || self
                .buttons
                .iter()
                .any(|label| !valid_text(label, MAX_MODAL_BUTTON_BYTES))
            || matches!(&self.initial_text, ROption::RSome(text) if !valid_text(text, MAX_MODAL_TEXT_BYTES))
        {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_MODAL_REQUEST",
                "modal request text or button bounds are invalid",
            ));
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub enum ModalResponse {
    /// Window dismissal or session cancellation is never a default button answer.
    Cancelled,
    Submitted {
        button_index: u32,
        text: ROption<RString>,
    },
}

impl ModalResponse {
    pub fn validate_for_request(&self, request: &ModalRequest) -> FamilyResult<()> {
        request.validate()?;
        if let Self::Submitted { button_index, text } = self {
            if *button_index as usize >= request.buttons.len()
                || text.is_some() != request.initial_text.is_some()
                || matches!(text, ROption::RSome(text) if !valid_text(text, MAX_MODAL_TEXT_BYTES))
            {
                return Err(FamilyError::invalid(
                    "ASTRA_EMU_FAMILY_MODAL_RESPONSE",
                    "modal response does not match its request",
                ));
            }
        }
        Ok(())
    }
}

/// A fresh session-scoped service. Only core workers may wait on request;
/// the window thread must remain available to present, respond and cancel.
#[sabi_trait]
pub trait ModalService: Send + Sync {
    fn request(&self, request: ModalRequest) -> FfiFamilyResult<ModalResponse>;
    fn is_cancelled(&self) -> bool;
    /// One-shot cancellation wakes every blocked request. Late answers cannot
    /// be accepted after cancellation or attached to a subsequent session.
    fn cancel(&self) -> FfiFamilyResult<()>;
}

pub type ModalServiceBox = ModalService_TO<'static, RBox<()>>;

#[cfg(test)]
#[path = "modal_tests.rs"]
mod tests;
