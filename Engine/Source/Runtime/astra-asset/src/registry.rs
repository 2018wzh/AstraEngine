use std::collections::BTreeMap;

use astra_core::Diagnostic;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{AssetError, AssetId, AssetSidecar};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AssetRecord {
    pub sidecar: AssetSidecar,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AssetRegistry {
    assets: BTreeMap<AssetId, AssetRecord>,
}

impl AssetRegistry {
    pub fn insert(&mut self, sidecar: AssetSidecar) -> Result<(), AssetError> {
        let mut diagnostics = sidecar.validate();
        if self.assets.contains_key(&sidecar.id) {
            diagnostics.push(
                Diagnostic::blocking("ASTRA_ASSET_DUPLICATE_ID", "duplicate AssetId")
                    .with_field("asset_id", sidecar.id.as_str()),
            );
        }
        if !diagnostics.is_empty() {
            return Err(AssetError::Diagnostics(diagnostics));
        }
        self.assets
            .insert(sidecar.id.clone(), AssetRecord { sidecar });
        Ok(())
    }

    pub fn get(&self, id: &AssetId) -> Option<&AssetRecord> {
        self.assets.get(id)
    }

    pub fn records(&self) -> impl Iterator<Item = &AssetRecord> {
        self.assets.values()
    }
}
