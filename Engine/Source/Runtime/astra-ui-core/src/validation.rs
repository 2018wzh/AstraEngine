use thiserror::Error;

pub const MAX_TREE_DEPTH: usize = 32;
pub const MAX_NODES_PER_VIEW: usize = 4096;
pub const MAX_COMPONENT_INSTANCES_PER_VIEW: usize = 1024;
pub const MAX_DTO_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_SESSION_STATE_BYTES: usize = 1024 * 1024;
pub const MAX_EFFECTS_PER_CALL: usize = 256;
pub const MAX_WEB_MEMORY_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_STRING_BYTES: usize = 16 * 1024;
pub const MAX_VERTICES_PER_FRAME: usize = 250_000;
pub const MAX_INDICES_PER_FRAME: usize = 750_000;
pub const MAX_TEXTURE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_DRAW_CALLS: usize = 128;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum UiValidationError {
    #[error("{code}: {message}")]
    Invalid { code: &'static str, message: String },
}

impl UiValidationError {
    pub fn invalid(code: &'static str, message: impl Into<String>) -> Self {
        Self::Invalid {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid { code, .. } => code,
        }
    }
}

pub trait ValidateUi {
    fn validate(&self) -> Result<(), UiValidationError>;
}

pub(crate) fn validate_id(field: &'static str, value: &str) -> Result<(), UiValidationError> {
    validate_string(field, value)?;
    if value.trim().is_empty() {
        return Err(UiValidationError::invalid(
            "ASTRA_UI_ID_EMPTY",
            format!("{field} must not be empty"),
        ));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/' | ':'))
    {
        return Err(UiValidationError::invalid(
            "ASTRA_UI_ID_UNSAFE",
            format!("{field} contains an unsupported character"),
        ));
    }
    Ok(())
}

pub(crate) fn validate_string(field: &'static str, value: &str) -> Result<(), UiValidationError> {
    if value.len() > MAX_STRING_BYTES {
        return Err(UiValidationError::invalid(
            "ASTRA_UI_STRING_LIMIT",
            format!("{field} exceeds {MAX_STRING_BYTES} UTF-8 bytes"),
        ));
    }
    if value.chars().any(|ch| ch == '\0') {
        return Err(UiValidationError::invalid(
            "ASTRA_UI_STRING_NUL",
            format!("{field} contains NUL"),
        ));
    }
    Ok(())
}

pub fn validate_serialized_size<T: serde::Serialize>(value: &T) -> Result<(), UiValidationError> {
    let serialized_size = postcard::experimental::serialized_size(value)
        .map_err(|error| UiValidationError::invalid("ASTRA_UI_DTO_ENCODE", error.to_string()))?;
    if serialized_size > MAX_DTO_BYTES {
        return Err(UiValidationError::invalid(
            "ASTRA_UI_DTO_LIMIT",
            format!(
                "serialized DTO is {} bytes; limit is {MAX_DTO_BYTES}",
                serialized_size
            ),
        ));
    }
    Ok(())
}
