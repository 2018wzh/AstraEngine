use std::fmt;

use abi_stable::{
    std_types::{RString, RVec},
    StableAbi,
};

use crate::FAMILY_ABI_FINGERPRINT;

pub const MAX_SYMBOL_BYTES: usize = 256;
pub const MAX_GAME_PATH_BYTES: usize = 4096;
pub const MAX_EVENTS_PER_ADVANCE: usize = 4096;
pub const MAX_TEXT_BYTES: usize = 32 * 1024;

/// Error crossing the family boundary. The code is stable; the message is
/// for a human and must not contain game text, secrets, or host paths.
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FamilyError {
    pub code: RString,
    pub message: RString,
}

impl FamilyError {
    pub fn new(code: impl Into<RString>, message: impl Into<RString>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn invalid(code: impl Into<RString>, message: impl Into<RString>) -> Self {
        Self::new(code, message)
    }

    pub fn code(&self) -> &str {
        self.code.as_str()
    }
}

impl fmt::Display for FamilyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for FamilyError {}

pub type FamilyResult<T> = Result<T, FamilyError>;
pub type FfiFamilyResult<T> = abi_stable::std_types::RResult<T, FamilyError>;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FamilyCapability {
    CpuFrame,
    PcmAudio,
    NativeSave,
    TextReplacement,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FamilyDescriptor {
    pub family_id: RString,
    pub plugin_id: RString,
    pub abi_fingerprint: RString,
    pub version: RString,
    pub capabilities: RVec<FamilyCapability>,
    pub supported_formats: RVec<RString>,
}

impl FamilyDescriptor {
    pub fn validate(&self) -> FamilyResult<()> {
        validate_symbol("family_id", &self.family_id)?;
        validate_symbol("plugin_id", &self.plugin_id)?;
        validate_symbol("abi_fingerprint", &self.abi_fingerprint)?;
        validate_symbol("version", &self.version)?;
        if self.abi_fingerprint != FAMILY_ABI_FINGERPRINT {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_ABI_FINGERPRINT",
                format!("unsupported family ABI; expected {FAMILY_ABI_FINGERPRINT}"),
            ));
        }
        if self.capabilities.is_empty() || !self.capabilities.contains(&FamilyCapability::CpuFrame)
        {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_CAPABILITIES",
                "family must declare CpuFrame capability",
            ));
        }
        for (index, capability) in self.capabilities.iter().enumerate() {
            if self.capabilities[..index].contains(capability) {
                return Err(FamilyError::invalid(
                    "ASTRA_EMU_FAMILY_DUPLICATE_CAPABILITY",
                    "family descriptor contains a duplicate capability",
                ));
            }
        }
        validate_unique_symbols("supported_formats", &self.supported_formats)?;
        if self.supported_formats.is_empty() {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_FORMATS",
                "family must declare at least one supported game format",
            ));
        }
        Ok(())
    }

    pub fn has_capability(&self, capability: FamilyCapability) -> bool {
        self.capabilities.contains(&capability)
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct ProbeRequest {
    /// The only game resource crossing the contract. The family owns all
    /// reads below this directory and must not receive a host VFS object.
    pub game_path: RString,
}

impl ProbeRequest {
    pub fn validate(&self) -> FamilyResult<()> {
        validate_game_path(&self.game_path)
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct ProbeReport {
    pub family_id: RString,
    /// Opaque provider-defined ID. It may contain Unicode and is not used as
    /// a log field; a no-match is represented by `ROption<ProbeReport>`.
    pub game_id: RString,
    pub format: RString,
    pub confidence_permyriad: u16,
}

impl ProbeReport {
    pub fn validate(&self) -> FamilyResult<()> {
        validate_symbol("family_id", &self.family_id)?;
        if self.game_id.is_empty() || self.game_id.len() > MAX_SYMBOL_BYTES {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_GAME_ID",
                "game ID must be non-empty and within the ID bound",
            ));
        }
        validate_symbol("format", &self.format)?;
        if self.confidence_permyriad > 10_000 {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_PROBE_CONFIDENCE",
                "probe confidence exceeds 10000 permyriad",
            ));
        }
        Ok(())
    }
}

pub(crate) fn validate_symbol(name: &str, value: &str) -> FamilyResult<()> {
    if value.is_empty() || value.len() > MAX_SYMBOL_BYTES || !value.is_ascii() {
        return Err(FamilyError::invalid(
            "ASTRA_EMU_FAMILY_SYMBOL",
            format!("{name} must be non-empty ASCII within the symbol bound"),
        ));
    }
    Ok(())
}

pub(crate) fn validate_unique_symbols(name: &str, values: &[RString]) -> FamilyResult<()> {
    for (index, value) in values.iter().enumerate() {
        validate_symbol(&format!("{name}[{index}]"), value)?;
        if values[..index].iter().any(|other| other == value) {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_DUPLICATE",
                format!("{name} contains a duplicate symbol"),
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_game_path(path: &str) -> FamilyResult<()> {
    if path.is_empty() || path.len() > MAX_GAME_PATH_BYTES || path.contains('\0') {
        return Err(FamilyError::invalid(
            "ASTRA_EMU_FAMILY_GAME_PATH",
            "game path is empty, too long, or contains NUL",
        ));
    }
    Ok(())
}
