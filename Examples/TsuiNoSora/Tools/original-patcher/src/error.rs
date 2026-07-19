use std::{fmt, io};

use serde::Serialize;
use thiserror::Error;

pub type PatchResult<T> = Result<T, PatchError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    Validation,
    Helper,
    Io,
}

#[derive(Debug, Error)]
#[error("{code}: {message}")]
pub struct PatchError {
    pub code: &'static str,
    pub message: String,
    pub class: ErrorClass,
}

impl PatchError {
    pub fn validation(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            class: ErrorClass::Validation,
        }
    }

    pub fn helper(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            class: ErrorClass::Helper,
        }
    }

    pub fn io(code: &'static str, action: &'static str, source: io::Error) -> Self {
        Self {
            code,
            message: format!("{action}: {source}"),
            class: ErrorClass::Io,
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self.class {
            ErrorClass::Validation => 2,
            ErrorClass::Helper => 3,
            ErrorClass::Io => 4,
        }
    }
}

impl From<serde_json::Error> for PatchError {
    fn from(error: serde_json::Error) -> Self {
        Self::validation("TSUI_PATCH_JSON_INVALID", error.to_string())
    }
}

impl From<walkdir::Error> for PatchError {
    fn from(error: walkdir::Error) -> Self {
        let message = match error.io_error() {
            Some(io_error) => format!("walk game directory: {io_error}"),
            None => "walk game directory failed".to_owned(),
        };
        Self {
            code: "TSUI_PATCH_DIRECTORY_WALK_FAILED",
            message,
            class: ErrorClass::Io,
        }
    }
}

impl fmt::Display for ErrorClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Validation => "validation",
            Self::Helper => "helper",
            Self::Io => "io",
        })
    }
}
