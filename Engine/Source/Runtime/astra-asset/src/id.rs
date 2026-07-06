use std::{fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AssetError {
    #[error("{0}")]
    Message(String),
    #[error("asset validation blocked")]
    Diagnostics(Vec<astra_core::Diagnostic>),
}

impl AssetError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    pub fn diagnostics(&self) -> &[astra_core::Diagnostic] {
        match self {
            Self::Diagnostics(diagnostics) => diagnostics,
            Self::Message(_) => &[],
        }
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(try_from = "String", into = "String")]
pub struct AssetId(String);

impl AssetId {
    pub fn parse(value: &str) -> Result<Self, AssetError> {
        if !value.starts_with("asset:/") {
            return Err(AssetError::message("AssetId must start with asset:/"));
        }
        let path = &value["asset:/".len()..];
        if path.is_empty()
            || path.starts_with('/')
            || path.ends_with('/')
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(AssetError::message(
                "AssetId contains an invalid asset path",
            ));
        }
        if path.chars().any(|ch| ch == '\\' || ch.is_control()) {
            return Err(AssetError::message("AssetId contains an invalid character"));
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for AssetId {
    type Err = AssetError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl TryFrom<String> for AssetId {
    type Error = AssetError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<AssetId> for String {
    fn from(value: AssetId) -> Self {
        value.0
    }
}

pub fn normalize_source_path(path: &str) -> Result<String, AssetError> {
    let replaced = path.replace('\\', "/");
    if replaced.is_empty()
        || replaced.starts_with('/')
        || replaced.starts_with("~/")
        || replaced.contains("://")
        || replaced
            .split('/')
            .next()
            .is_some_and(|part| part.ends_with(':'))
    {
        return Err(AssetError::message("source path must be project-relative"));
    }

    let mut parts = Vec::new();
    for part in replaced.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(AssetError::message(
                "source path cannot escape project root",
            ));
        }
        if part.chars().any(|ch| ch.is_control()) {
            return Err(AssetError::message(
                "source path contains control character",
            ));
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(AssetError::message("source path cannot be empty"));
    }
    Ok(parts.join("/"))
}
