use std::{path::PathBuf, sync::Arc};

use astra_core::Hash256;

use crate::{LegacyCoreError, LegacyMountedVfs};

#[derive(Debug, Clone)]
pub struct LegacyOpaqueFamilyConfig {
    pub schema_id: String,
    pub schema_hash: Hash256,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct LegacyVfsMountContext {
    pub game_root: PathBuf,
    pub profile_id: String,
    pub profile_hash: Hash256,
    pub mount_id: String,
    pub prefix: String,
    pub private_patch: Option<PathBuf>,
    pub family_config: LegacyOpaqueFamilyConfig,
}

pub trait LegacyVfsFamilyFactory: Send + Sync {
    fn family_id(&self) -> &str;
    fn mount_profile_schema_id(&self) -> &str;
    fn mount_profile_schema_hash(&self) -> Hash256;
    fn decrypt_provider_id(&self) -> &str;
    fn mount(
        &self,
        context: &LegacyVfsMountContext,
    ) -> Result<Arc<dyn LegacyMountedVfs>, LegacyCoreError>;
}
