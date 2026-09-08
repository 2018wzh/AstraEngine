#![allow(non_local_definitions)]

use abi_stable::{
    sabi_trait,
    std_types::{RBox, RString},
    StableAbi,
};

use super::descriptor::{
    validate_symbol, FamilyError, FamilyResult, FfiFamilyResult, MAX_SYMBOL_BYTES, MAX_TEXT_BYTES,
};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum TextResetReason {
    NewGame,
    Load,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct TextReplacementRequest {
    pub request_id: RString,
    pub source: RString,
    pub speaker: RString,
    pub ruby: RString,
}

impl TextReplacementRequest {
    pub fn validate(&self) -> FamilyResult<()> {
        validate_symbol("request_id", &self.request_id)?;
        if self.source.len() > MAX_TEXT_BYTES
            || self.speaker.len() > MAX_SYMBOL_BYTES
            || self.ruby.len() > MAX_TEXT_BYTES
        {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_TEXT_BOUNDS",
                "text replacement request exceeds the ABI bound",
            ));
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct TextReplacementResponse {
    pub request_id: RString,
    pub replacement: RString,
}

impl TextReplacementResponse {
    pub fn validate(&self) -> FamilyResult<()> {
        validate_symbol("request_id", &self.request_id)?;
        if self.replacement.len() > MAX_TEXT_BYTES {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_TEXT_BOUNDS",
                "text replacement exceeds the ABI bound",
            ));
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub enum TextPollResult {
    Pending,
    Ready(TextReplacementResponse),
    Cancelled,
    Failed(FamilyError),
}

#[sabi_trait]
pub trait TextReplacementService: Send + Sync {
    fn reset(&self, reason: TextResetReason) -> FfiFamilyResult<()>;
    fn submit(&self, request: TextReplacementRequest) -> FfiFamilyResult<()>;
    fn poll(&self, request_id: RString) -> FfiFamilyResult<TextPollResult>;
    fn cancel(&self, request_id: RString) -> FfiFamilyResult<()>;
}

pub type TextReplacementServiceBox = TextReplacementService_TO<'static, RBox<()>>;
