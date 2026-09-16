#![allow(non_local_definitions)]

use abi_stable::{
    sabi_trait,
    std_types::{RBox, RString, RVec},
    StableAbi,
};

use crate::{FamilyError, FamilyResult};

pub const MAX_DIAGNOSTIC_FIELDS: usize = 32;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum DiagnosticLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, StableAbi)]
pub enum DiagnosticValue {
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    Number(f64),
    Symbol(RString),
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, StableAbi)]
pub struct DiagnosticField {
    pub name: RString,
    pub value: DiagnosticValue,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, StableAbi)]
pub struct DiagnosticEvent {
    pub level: DiagnosticLevel,
    pub target: RString,
    pub event: RString,
    pub fields: RVec<DiagnosticField>,
    /// Unreviewed text, Debug values and excess fields are never forwarded.
    pub redacted_fields: u32,
}

pub fn diagnostic_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
}

impl DiagnosticEvent {
    pub fn validate(&self) -> FamilyResult<()> {
        let valid = diagnostic_symbol(&self.target)
            && diagnostic_symbol(&self.event)
            && self.fields.len() <= MAX_DIAGNOSTIC_FIELDS
            && self.fields.iter().enumerate().all(|(index, field)| {
                diagnostic_symbol(&field.name)
                    && !self.fields[..index]
                        .iter()
                        .any(|previous| previous.name == field.name)
                    && match &field.value {
                        DiagnosticValue::Symbol(value) => diagnostic_symbol(value),
                        DiagnosticValue::Number(value) => value.is_finite(),
                        _ => true,
                    }
            });
        if !valid {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_DIAGNOSTIC",
                "invalid bounded family diagnostic",
            ));
        }
        Ok(())
    }
}

/// Process-lifetime host service. No session, UI or device resources may be
/// captured. Calls can originate on any core worker; do not call the family back.
#[sabi_trait]
pub trait DiagnosticSink: Send + Sync {
    fn enabled(&self, level: DiagnosticLevel) -> bool;
    fn emit(&self, event: DiagnosticEvent);
}

pub type DiagnosticSinkBox = DiagnosticSink_TO<'static, RBox<()>>;
