use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use astra_core::Hash256;
use astra_emu_family_core::{
    LegacyCoreError, LegacyMountedVfs, LegacyVfsFamilyFactory, LegacyVfsMountContext,
};
use astra_emu_family_support::{load_private_profile, PlaintextCache};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    MinoriMountedVfs, MinoriPazDecryptProvider, PazArchiveConfig, PazRoleScheme,
    MINORI_DECRYPT_PROVIDER_ID, MINORI_FAMILY_OPTIONS_SCHEMA, MINORI_PRIVATE_PROFILE_SCHEMA,
    REQUIRED_ARCHIVE_ROLES,
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MinoriFamilyOptions {
    pub paz_version: u8,
    pub index_size_xor: u32,
    pub private_profile_id: String,
    pub private_profile_schema: String,
    pub cache: MinoriCacheOptions,
    pub archive_roles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MinoriCacheOptions {
    pub enabled: bool,
    pub total_bytes: u64,
    pub entry_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MinoriPrivateProfilePayload {
    pub schema: String,
    pub roles: BTreeMap<String, MinoriRolePrivateProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MinoriRolePrivateProfile {
    pub index_key: Vec<u8>,
    pub data_key: Vec<u8>,
    pub type_passwords: BTreeMap<String, String>,
    pub archive_xor: Option<u32>,
    pub video_key: Option<Vec<u8>>,
}

#[derive(Debug, Default)]
pub struct MinoriVfsFamilyFactory;

impl LegacyVfsFamilyFactory for MinoriVfsFamilyFactory {
    fn family_id(&self) -> &str {
        "minori"
    }
    fn mount_profile_schema_id(&self) -> &str {
        MINORI_FAMILY_OPTIONS_SCHEMA
    }
    fn mount_profile_schema_hash(&self) -> Hash256 {
        Hash256::from_sha256(MINORI_FAMILY_OPTIONS_SCHEMA.as_bytes())
    }
    fn decrypt_provider_id(&self) -> &str {
        MINORI_DECRYPT_PROVIDER_ID
    }

    fn mount(
        &self,
        context: &LegacyVfsMountContext,
    ) -> Result<Arc<dyn LegacyMountedVfs>, LegacyCoreError> {
        if context.family_config.schema_id != MINORI_FAMILY_OPTIONS_SCHEMA
            || context.family_config.schema_hash != self.mount_profile_schema_hash()
            || context.prefix != "minori:/"
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_MOUNT_PROFILE",
                "Minori mount identity or options schema is invalid",
            ));
        }
        let options: MinoriFamilyOptions = serde_json::from_slice(&context.family_config.payload)
            .map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_MOUNT_OPTIONS",
                "Minori family options are invalid",
            )
        })?;
        validate_options(&options)?;
        let patch = context.private_patch.as_deref().ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_PRIVATE_PATCH",
                "Minori requires a trusted private patch",
            )
        })?;
        let private = load_private_profile(
            patch,
            &options.private_profile_id,
            &options.private_profile_schema,
        )?;
        if private.schema_id != MINORI_PRIVATE_PROFILE_SCHEMA
            || private.schema_hash != Hash256::from_sha256(MINORI_PRIVATE_PROFILE_SCHEMA.as_bytes())
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_PRIVATE_SCHEMA",
                "Minori private profile schema is invalid",
            ));
        }
        let payload: MinoriPrivateProfilePayload = serde_json::from_slice(private.payload())
            .map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_PRIVATE_PAYLOAD",
                    "Minori private profile payload is invalid",
                )
            })?;
        let schemes = payload.into_schemes(options.index_size_xor)?;
        let decrypt_provider = Arc::new(MinoriPazDecryptProvider::new(
            private.payload_hash,
            schemes,
        )?);
        let configs = options
            .archive_roles
            .iter()
            .map(|role| PazArchiveConfig {
                role: role.clone(),
                path: context.game_root.join(format!("{role}.paz")),
                game_root: context.game_root.clone(),
                version: options.paz_version,
                index_size_xor: options.index_size_xor,
            })
            .collect();
        let cache = if options.cache.enabled {
            let root = directories::ProjectDirs::from("org", "AstraEngine", "AstraEMU")
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_MINORI_CACHE_ROOT",
                        "platform private cache root is unavailable",
                    )
                })?
                .cache_dir()
                .join("family")
                .join("minori")
                .join(context.profile_hash.to_hex());
            Some(
                PlaintextCache::new(root, options.cache.total_bytes, options.cache.entry_bytes)
                    .map_err(|_| {
                        invalid(
                            "ASTRA_EMU_MINORI_CACHE_INIT",
                            "Minori plaintext cache initialization failed",
                        )
                    })?,
            )
        } else {
            None
        };
        Ok(Arc::new(MinoriMountedVfs::mount_with_cache(
            context.mount_id.clone(),
            context.prefix.clone(),
            configs,
            decrypt_provider,
            context.profile_hash,
            cache,
        )?))
    }
}

impl MinoriPrivateProfilePayload {
    fn into_schemes(
        self,
        expected_xor: u32,
    ) -> Result<BTreeMap<String, PazRoleScheme>, LegacyCoreError> {
        if self.schema != MINORI_PRIVATE_PROFILE_SCHEMA {
            return Err(invalid(
                "ASTRA_EMU_MINORI_PRIVATE_SCHEMA",
                "Minori private payload schema is invalid",
            ));
        }
        let roles = self
            .roles
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if roles != REQUIRED_ARCHIVE_ROLES.into_iter().collect() {
            return Err(invalid(
                "ASTRA_EMU_MINORI_PRIVATE_ROLES",
                "Minori private payload does not contain exactly six archive roles",
            ));
        }
        self.roles
            .into_iter()
            .map(|(role, value)| {
                if value.archive_xor.is_some_and(|xor| xor != expected_xor) {
                    return Err(invalid(
                        "ASTRA_EMU_MINORI_PRIVATE_XOR",
                        "Minori private archive XOR does not match mount options",
                    ));
                }
                let video_key = value
                    .video_key
                    .map(|key| {
                        key.try_into().map_err(|_| {
                            invalid(
                                "ASTRA_EMU_MINORI_PRIVATE_VIDEO_KEY",
                                "Minori video key must contain exactly 256 bytes",
                            )
                        })
                    })
                    .transpose()?;
                if value
                    .type_passwords
                    .keys()
                    .any(|key| !matches!(key.as_str(), "png" | "ogg" | "sc" | "avi"))
                {
                    return Err(invalid(
                        "ASTRA_EMU_MINORI_PRIVATE_TYPE_KEY",
                        "Minori private payload contains an unknown type key",
                    ));
                }
                Ok((
                    role,
                    PazRoleScheme {
                        index_key: value.index_key,
                        data_key: value.data_key,
                        type_passwords: value.type_passwords,
                        archive_xor: value.archive_xor,
                        video_key,
                    },
                ))
            })
            .collect()
    }
}

fn validate_options(options: &MinoriFamilyOptions) -> Result<(), LegacyCoreError> {
    let roles = options
        .archive_roles
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if options.paz_version > 2
        || options.private_profile_schema != MINORI_PRIVATE_PROFILE_SCHEMA
        || options.private_profile_id.is_empty()
        || roles.len() != options.archive_roles.len()
        || roles != REQUIRED_ARCHIVE_ROLES.into_iter().collect()
        || options.cache.enabled
            && (options.cache.entry_bytes == 0
                || options.cache.total_bytes < options.cache.entry_bytes)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_MOUNT_OPTIONS",
            "Minori family options violate their contract",
        ));
    }
    Ok(())
}

fn invalid(code: &'static str, message: &'static str) -> LegacyCoreError {
    LegacyCoreError::invalid(code, message)
}
