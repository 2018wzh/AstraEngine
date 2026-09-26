//! Explicit policy continuations; no Lua VM, coroutine or native handle is persisted.
use crate::VnError;
use astra_asset::VfsUri;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    pin::Pin,
};

pub const POLICY_IO_MAX_REQUESTS: usize = 64;
pub const POLICY_IO_MAX_BYTES: usize = 1024 * 1024;

/// Granted by the embedding host, never by a package or script.
#[derive(Debug, Clone, Default)]
pub struct PolicyIoCapabilities {
    resources: BTreeSet<VfsUri>,
    max_bytes: usize,
}
impl PolicyIoCapabilities {
    pub fn read_text(
        resources: impl IntoIterator<Item = VfsUri>,
        max_bytes: usize,
    ) -> Result<Self, VnError> {
        if max_bytes == 0 || max_bytes > POLICY_IO_MAX_BYTES {
            return Err(io_error("invalid read limit"));
        }
        let resources: BTreeSet<_> = resources.into_iter().collect();
        if resources.len() > POLICY_IO_MAX_REQUESTS {
            return Err(io_error("too many granted resources"));
        }
        for resource in &resources {
            validate_resource(resource)?;
        }
        Ok(Self {
            resources,
            max_bytes,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PolicyReadText {
    pub id: u64,
    pub resource: VfsUri,
    pub max_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PolicyIoFailure {
    Host,
    Limit,
    Cancelled,
    Interrupted,
}

/// Host implementations must return immediately and provide a cancellation-safe future.
/// Dropping the future must release IO resources; detached and blocking workers are forbidden.
pub trait PolicyIoHost: Send + Sync + 'static {
    fn read_text(
        &self,
        request: PolicyReadText,
    ) -> Pin<Box<dyn Future<Output = Result<String, PolicyIoFailure>> + Send>>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PolicyIoStatus {
    Pending,
    Ready(String),
    Failed(PolicyIoFailure),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PolicyIoRecord {
    pub request: PolicyReadText,
    pub status: PolicyIoStatus,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PolicyIoState {
    pub next_id: u64,
    pub requests: BTreeMap<String, PolicyIoRecord>,
}
impl Default for PolicyIoState {
    fn default() -> Self {
        Self {
            next_id: 1,
            requests: BTreeMap::new(),
        }
    }
}
impl PolicyIoState {
    pub fn validate(&self) -> Result<(), VnError> {
        if self.next_id == 0 || self.requests.len() > POLICY_IO_MAX_REQUESTS {
            return Err(io_error("invalid request state"));
        }
        let mut ids = BTreeSet::new();
        for (key, record) in &self.requests {
            validate_key(key)?;
            validate_resource(&record.request.resource)?;
            let request = &record.request;
            if request.id == 0
                || request.id >= self.next_id
                || !ids.insert(request.id)
                || request.max_bytes == 0
                || request.max_bytes > POLICY_IO_MAX_BYTES
                || VfsUri::parse(request.resource.as_str()).is_err()
                || matches!(&record.status, PolicyIoStatus::Ready(text) if text.len() > request.max_bytes)
            {
                return Err(io_error("invalid saved IO record"));
            }
        }
        Ok(())
    }
    pub fn read_text(
        &mut self,
        key: String,
        resource: VfsUri,
        capabilities: &PolicyIoCapabilities,
    ) -> Result<PolicyIoStatus, VnError> {
        validate_key(&key)?;
        validate_resource(&resource)?;
        if let Some(record) = self.requests.get(&key) {
            if record.request.resource != resource {
                return Err(io_error("request key refers to a different resource"));
            }
            return Ok(record.status.clone());
        }
        if !capabilities.resources.contains(&resource) {
            return Err(VnError::diagnostic(
                "ASTRA_POLICY_HOST_IO_DENIED",
                "resource was not granted by the host",
            ));
        }
        if self.requests.len() >= POLICY_IO_MAX_REQUESTS {
            return Err(io_error(
                "request capacity exceeded; release completed requests",
            ));
        }
        let next = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| io_error("request sequence overflow"))?;
        self.requests.insert(
            key,
            PolicyIoRecord {
                request: PolicyReadText {
                    id: self.next_id,
                    resource,
                    max_bytes: capabilities.max_bytes,
                },
                status: PolicyIoStatus::Pending,
            },
        );
        self.next_id = next;
        Ok(PolicyIoStatus::Pending)
    }
    pub fn interrupt_pending(&mut self) {
        for record in self.requests.values_mut() {
            if record.status == PolicyIoStatus::Pending {
                record.status = PolicyIoStatus::Failed(PolicyIoFailure::Interrupted);
            }
        }
    }
}
fn validate_key(key: &str) -> Result<(), VnError> {
    if key.is_empty()
        || key.len() > 128
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(io_error("invalid request key"));
    }
    Ok(())
}
fn io_error(message: &str) -> VnError {
    VnError::diagnostic("ASTRA_POLICY_HOST_IO_STATE", message)
}

pub struct PolicyIoContext<'a> {
    pub state: &'a mut PolicyIoState,
    pub capabilities: &'a PolicyIoCapabilities,
}

fn validate_resource(resource: &VfsUri) -> Result<(), VnError> {
    if resource.as_str().len() > 1024 || resource.prefix().len() == 1 || resource.prefix() == "file"
    {
        return Err(io_error(
            "host IO uses logical mounted resources, not native paths",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grants_reject_native_paths_and_invalid_limits() {
        for resource in ["file:/private/data", "C:/private/data", "c:/private/data"] {
            let rejected = match VfsUri::parse(resource) {
                Err(_) => true,
                Ok(uri) => PolicyIoCapabilities::read_text([uri], 64).is_err(),
            };
            assert!(rejected);
        }
        for limit in [0, POLICY_IO_MAX_BYTES + 1] {
            assert!(PolicyIoCapabilities::read_text(
                [VfsUri::parse("package:/data").unwrap()],
                limit
            )
            .is_err());
        }
    }
    #[test]
    fn saved_requests_are_bounded_and_reject_duplicate_identity() {
        let resource = VfsUri::parse("package:/data").unwrap();
        let grant = PolicyIoCapabilities::read_text([resource.clone()], 64).unwrap();
        let mut state = PolicyIoState::default();
        for id in 0..POLICY_IO_MAX_REQUESTS {
            state
                .read_text(format!("read.{id}"), resource.clone(), &grant)
                .unwrap();
        }
        assert!(state
            .read_text("overflow".into(), resource, &grant)
            .is_err());
        let mut restored: PolicyIoState =
            postcard::from_bytes(&postcard::to_allocvec(&state).unwrap()).unwrap();
        restored.validate().unwrap();
        let mut records = restored.requests.values_mut();
        let first_id = records.next().unwrap().request.id;
        records.next().unwrap().request.id = first_id;
        assert!(restored.validate().is_err());
    }
}
