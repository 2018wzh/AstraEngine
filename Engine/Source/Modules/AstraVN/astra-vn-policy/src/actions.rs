//! Package-owned registrations and explicit, stack-free story action continuations.
use crate::{
    ExtensionCommandDescriptor, ExtensionPresentationCommand, ExtensionValue, PolicySnapshotValue,
    VnError,
};
use astra_asset::VfsUri;
use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::POLICY_ACTION_PROVIDER;
pub(crate) const LUAU_MAX_EXACT_INTEGER: u64 = (1_u64 << 53) - 1;
pub const POLICY_ACTION_SECTION: &str = "vn.policy_actions";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PolicyActionBinding {
    pub descriptor: ExtensionCommandDescriptor,
    pub entry: String,
    pub source_hash: String,
    pub source: String,
    pub read_text: Vec<VfsUri>,
}

impl PolicyActionBinding {
    pub fn validate(&self) -> Result<(), VnError> {
        self.descriptor
            .validate()
            .map_err(|d| VnError::diagnostic(d.code, d.message))?;
        if self.descriptor.provider_id != POLICY_ACTION_PROVIDER
            || self.entry.is_empty()
            || self.entry.starts_with('/')
            || self.entry.contains(['\\', ':'])
            || self
                .entry
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
            || !self.entry.ends_with(".luau")
            || self.source.len() > crate::POLICY_IO_MAX_BYTES
            || self.source_hash != Hash256::from_sha256(self.source.as_bytes()).to_string()
        {
            return Err(action_error("invalid package action binding"));
        }
        crate::PolicyIoCapabilities::read_text(self.read_text.clone(), crate::POLICY_IO_MAX_BYTES)?;
        if self
            .read_text
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != self.read_text.len()
        {
            return Err(action_error("duplicate text grant"));
        }
        Ok(())
    }

    pub fn validate_command(&self, command: &ExtensionPresentationCommand) -> Result<(), VnError> {
        self.validate()?;
        let d = &self.descriptor;
        if command.command != d.command
            || command.provider_id != d.provider_id
            || command.schema != d.schema
        {
            return Err(action_error("action descriptor mismatch"));
        }
        for (name, field) in &d.fields {
            let Some(value) = command.fields.get(name) else {
                if field.required {
                    return Err(action_error("required action argument missing"));
                }
                continue;
            };
            use crate::ExtensionFieldKind as K;
            let matches = matches!(
                (field.kind, value),
                (K::String, ExtensionValue::String(_))
                    | (K::Integer, ExtensionValue::Integer(_))
                    | (K::Fixed, ExtensionValue::Fixed(_))
                    | (K::Boolean, ExtensionValue::Boolean(_))
                    | (K::Symbol, ExtensionValue::Symbol(_))
                    | (K::AssetUri, ExtensionValue::AssetUri(_))
            );
            if !matches {
                return Err(action_error("action argument type mismatch"));
            }
            if matches!(value, ExtensionValue::Integer(i) if i.unsigned_abs() > LUAU_MAX_EXACT_INTEGER)
            {
                return Err(action_error("action integer exceeds exact Luau range"));
            }
        }
        if command
            .fields
            .keys()
            .any(|name| !d.fields.contains_key(name))
        {
            return Err(action_error("unknown action argument"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PolicyActionOutcome {
    Complete,
    Await { request_key: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PolicyActionContinuation {
    pub source_id: String,
    pub instance: u64,
    pub command: ExtensionPresentationCommand,
    pub waiting_key: Option<String>,
    pub state: BTreeMap<String, PolicySnapshotValue>,
}

impl PolicyActionContinuation {
    pub fn validate(&self) -> Result<(), VnError> {
        if self.source_id.is_empty() || self.source_id.len() > 1024 || self.state.len() > 256 {
            return Err(action_error("invalid continuation identity or state"));
        }
        self.request_key("validate")?;
        if let Some(key) = &self.waiting_key {
            self.request_key(key)?;
        }
        for value in self.state.values() {
            value
                .validate_depth(crate::PolicyExecutionBudget::default().snapshot_depth)
                .map_err(|_| action_error("continuation nesting exceeds budget"))?;
        }
        if postcard::to_allocvec(self)
            .map_err(|_| action_error("invalid continuation state"))?
            .len()
            > crate::POLICY_IO_MAX_BYTES
        {
            return Err(action_error("continuation exceeds budget"));
        }
        Ok(())
    }

    pub fn request_key(&self, local: &str) -> Result<String, VnError> {
        if self.instance == 0
            || local.is_empty()
            || local.len() > 100
            || !local
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(action_error("invalid action request key"));
        }
        Ok(format!("a{}.{}", self.instance, local))
    }
}

pub(crate) fn action_error(message: &str) -> VnError {
    VnError::diagnostic("ASTRA_VN_POLICY_ACTION", message)
}
