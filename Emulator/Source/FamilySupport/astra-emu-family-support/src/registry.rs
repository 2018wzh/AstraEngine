use std::{collections::BTreeMap, path::Path, sync::Arc};

use astra_emu_family_core::{LegacyCoreError, LegacyMountedVfs, LegacyVfsFamilyFactory};

use crate::{load_mount_profile, LoadedMountProfile};

#[derive(Default)]
pub struct LegacyVfsFamilyRegistry {
    factories: BTreeMap<String, Arc<dyn LegacyVfsFamilyFactory>>,
}

impl LegacyVfsFamilyRegistry {
    pub fn register(
        &mut self,
        factory: Arc<dyn LegacyVfsFamilyFactory>,
    ) -> Result<(), LegacyCoreError> {
        let family_id = factory.family_id();
        if !safe_symbol(family_id) || self.factories.contains_key(family_id) {
            return Err(LegacyCoreError::invalid(
                "ASTRA_EMU_VFS_FACTORY_DUPLICATE",
                "family factory id is invalid or already registered",
            ));
        }
        self.factories.insert(family_id.to_owned(), factory);
        Ok(())
    }

    pub fn load_profile(&self, path: &Path) -> Result<LoadedMountProfile, LegacyCoreError> {
        load_mount_profile(path)
    }

    pub fn mount(
        &self,
        requested_family: &str,
        game_root: &Path,
        loaded: &LoadedMountProfile,
    ) -> Result<Arc<dyn LegacyMountedVfs>, LegacyCoreError> {
        if requested_family != loaded.profile.family_id {
            return Err(LegacyCoreError::invalid(
                "ASTRA_EMU_VFS_FAMILY_MISMATCH",
                "requested family does not match the mount profile",
            ));
        }
        let factory = self.factories.get(requested_family).ok_or_else(|| {
            LegacyCoreError::invalid(
                "ASTRA_EMU_VFS_FACTORY_MISSING",
                "requested family factory is not registered",
            )
        })?;
        if factory.mount_profile_schema_id() != loaded.profile.family_options_schema
            || factory.mount_profile_schema_hash() != loaded.family_config.schema_hash
        {
            return Err(LegacyCoreError::invalid(
                "ASTRA_EMU_VFS_OPTIONS_SCHEMA",
                "family options schema does not match the factory",
            ));
        }
        let mounted = factory.mount(&loaded.mount_context(game_root)?)?;
        let manifest = mounted.manifest();
        if manifest.family_id != requested_family
            || manifest.mount_profile_hash != loaded.profile_hash
            || manifest.mount_id != loaded.profile.mount_id
            || manifest.prefix != loaded.profile.prefix
            || manifest.decrypt_provider_id != factory.decrypt_provider_id()
        {
            return Err(LegacyCoreError::invalid(
                "ASTRA_EMU_VFS_FACTORY_RESULT_IDENTITY",
                "mounted VFS identity does not match the selected factory and profile",
            ));
        }
        manifest.validate(10_000_000)?;
        Ok(mounted)
    }
}

fn safe_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use astra_core::Hash256;
    use astra_emu_family_core::{
        LegacyCoreError, LegacyMountedVfs, LegacyOpaqueFamilyConfig, LegacyVfsFamilyFactory,
        LegacyVfsMountContext,
    };

    use crate::{LegacyVfsMountProfile, LoadedMountProfile, VFS_MOUNT_PROFILE_SCHEMA};

    use super::LegacyVfsFamilyRegistry;

    struct FixtureFactory;

    impl LegacyVfsFamilyFactory for FixtureFactory {
        fn family_id(&self) -> &str {
            "fixture"
        }
        fn mount_profile_schema_id(&self) -> &str {
            "fixture.options.v1"
        }
        fn mount_profile_schema_hash(&self) -> Hash256 {
            Hash256::from_sha256(b"fixture.options.v1")
        }
        fn decrypt_provider_id(&self) -> &str {
            "fixture.decrypt.v1"
        }
        fn mount(
            &self,
            _context: &LegacyVfsMountContext,
        ) -> Result<Arc<dyn LegacyMountedVfs>, LegacyCoreError> {
            Err(LegacyCoreError::invalid(
                "ASTRA_EMU_VFS_TEST_FACTORY_CALLED",
                "test factory should not be called",
            ))
        }
    }

    fn loaded(family: &str, schema_hash: Hash256) -> LoadedMountProfile {
        LoadedMountProfile {
            profile: LegacyVfsMountProfile {
                schema: VFS_MOUNT_PROFILE_SCHEMA.into(),
                profile_id: "fixture-profile".into(),
                family_id: family.into(),
                mount_id: "fixture-mount".into(),
                prefix: "fixture:/".into(),
                private_patch: "private.luau".into(),
                family_options_schema: "fixture.options.v1".into(),
                family_options: serde_json::json!({}),
            },
            profile_hash: Hash256::from_sha256(b"profile"),
            family_config: LegacyOpaqueFamilyConfig {
                schema_id: "fixture.options.v1".into(),
                schema_hash,
                payload: b"{}".to_vec(),
            },
        }
    }

    #[test]
    fn duplicate_factory_family_and_schema_mismatch_are_blocking() {
        let mut registry = LegacyVfsFamilyRegistry::default();
        registry.register(Arc::new(FixtureFactory)).unwrap();
        assert_eq!(
            registry
                .register(Arc::new(FixtureFactory))
                .unwrap_err()
                .code(),
            "ASTRA_EMU_VFS_FACTORY_DUPLICATE"
        );
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("private.luau"), b"private").unwrap();
        assert_eq!(
            registry
                .mount(
                    "fixture",
                    temp.path(),
                    &loaded("other", Hash256::from_sha256(b"fixture.options.v1")),
                )
                .err()
                .unwrap()
                .code(),
            "ASTRA_EMU_VFS_FAMILY_MISMATCH"
        );
        assert_eq!(
            registry
                .mount(
                    "fixture",
                    temp.path(),
                    &loaded("fixture", Hash256::from_sha256(b"wrong")),
                )
                .err()
                .unwrap()
                .code(),
            "ASTRA_EMU_VFS_OPTIONS_SCHEMA"
        );
    }
}
