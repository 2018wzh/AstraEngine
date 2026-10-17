use std::fmt;
pub const MAX_SYMBOL_BYTES: usize = 256;
pub const MAX_GAME_PATH_BYTES: usize = 4096;
pub const MAX_INPUT_EVENTS: usize = 4096;
pub const MAX_TEXT_BYTES: usize = 32 * 1024;

/// Error crossing the family boundary. The code is stable; the message is
/// for a human and must not contain game text, secrets, or host paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyError {
    pub code: String,
    pub message: String,
}

impl FamilyError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn invalid(code: impl Into<String>, message: impl Into<String>) -> Self {
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeRequest {
    /// The only game resource crossing the contract. The family owns all
    /// reads below this directory and must not receive a host VFS object.
    pub game_path: String,
}

impl ProbeRequest {
    pub fn validate(&self) -> FamilyResult<()> {
        validate_game_path(&self.game_path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    pub family_id: String,
    /// Opaque provider-defined ID. It may contain Unicode and is not used as
    /// a log field; a no-match is represented by `Option<ProbeReport>`.
    pub game_id: String,
    pub format: String,
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

pub(crate) fn validate_game_path(path: &str) -> FamilyResult<()> {
    if path.is_empty() || path.len() > MAX_GAME_PATH_BYTES || path.contains('\0') {
        return Err(FamilyError::invalid(
            "ASTRA_EMU_FAMILY_GAME_PATH",
            "game path is empty, too long, or contains NUL",
        ));
    }
    Ok(())
}
