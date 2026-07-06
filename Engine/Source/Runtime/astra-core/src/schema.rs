use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub type SchemaId = String;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct SchemaVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl SchemaVersion {
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl Default for SchemaVersion {
    fn default() -> Self {
        Self::new(1, 0, 0)
    }
}

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("missing migration for {schema} from {from:?} to {to:?}")]
    Missing {
        schema: SchemaId,
        from: SchemaVersion,
        to: SchemaVersion,
    },
}

pub trait SchemaMigrator: Send + Sync {
    fn schema(&self) -> &str;
    #[allow(clippy::wrong_self_convention)]
    fn from_version(&self) -> SchemaVersion;
    fn to_version(&self) -> SchemaVersion;
    fn migrate(&self, bytes: &[u8]) -> Result<Vec<u8>, MigrationError>;
}

#[derive(Default)]
pub struct SchemaMigrationRegistry {
    edges: BTreeMap<SchemaId, BTreeSet<(SchemaVersion, SchemaVersion)>>,
}

impl SchemaMigrationRegistry {
    pub fn register_identity(
        &mut self,
        schema: impl Into<SchemaId>,
        from: SchemaVersion,
        to: SchemaVersion,
    ) {
        self.edges
            .entry(schema.into())
            .or_default()
            .insert((from, to));
    }

    pub fn validate_chain(
        &self,
        schema: &str,
        from: SchemaVersion,
        to: SchemaVersion,
    ) -> Result<(), MigrationError> {
        if from == to {
            return Ok(());
        }
        let Some(edges) = self.edges.get(schema) else {
            return Err(MigrationError::Missing {
                schema: schema.to_string(),
                from,
                to,
            });
        };
        let mut frontier = BTreeSet::from([from]);
        let mut visited = BTreeSet::new();
        while let Some(current) = frontier.pop_first() {
            if current == to {
                return Ok(());
            }
            if !visited.insert(current) {
                continue;
            }
            for (edge_from, edge_to) in edges {
                if *edge_from == current {
                    frontier.insert(*edge_to);
                }
            }
        }
        Err(MigrationError::Missing {
            schema: schema.to_string(),
            from,
            to,
        })
    }
}
