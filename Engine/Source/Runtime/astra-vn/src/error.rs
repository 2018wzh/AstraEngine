use astra_core::Diagnostic;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VnError {
    #[error("{0}")]
    Message(String),
    #[error("AstraVN diagnostic: {0:?}")]
    Diagnostic(Diagnostic),
    #[error("postcard codec failed: {0}")]
    Postcard(String),
    #[error("Luau policy failed: {0}")]
    Luau(String),
}

impl VnError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    pub fn diagnostic(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Diagnostic(Diagnostic::blocking(code, message))
    }

    pub fn code(&self) -> &str {
        match self {
            Self::Diagnostic(diagnostic) => &diagnostic.code,
            Self::Luau(_) => "ASTRA_VN_LUAU_SANDBOX",
            Self::Message(_) | Self::Postcard(_) => "ASTRA_VN_ERROR",
        }
    }
}

impl From<postcard::Error> for VnError {
    fn from(value: postcard::Error) -> Self {
        Self::Postcard(value.to_string())
    }
}
