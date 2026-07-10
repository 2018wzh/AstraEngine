use astra_core::Diagnostic;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("policy diagnostics blocked")]
    Diagnostic(Diagnostic),
    #[error("{0}")]
    Runtime(String),
}

impl PolicyError {
    pub fn diagnostic(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Diagnostic(Diagnostic::blocking(code, message))
    }

    pub fn code(&self) -> &str {
        match self {
            Self::Diagnostic(diagnostic) => &diagnostic.code,
            Self::Runtime(_) => "ASTRA_POLICY_RUNTIME",
        }
    }
}
