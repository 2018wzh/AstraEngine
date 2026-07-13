use astra_asset::{ResolveContext, VfsManifest};
use astra_plugin_abi::{
    PluginExtensionRegistrySnapshot, ProviderPolicy, ValidatedRuntimeProviderSelection,
};
use astra_target::{validate_manifest, TargetKind, TargetManifest, TargetValidationStatus};
use std::collections::BTreeSet;

use crate::ContainerError;

pub(crate) fn validate_provider_authority(
    package_id: &str,
    profile: &str,
    policy_bytes: &[u8],
    registry_bytes: &[u8],
    target_manifest_bytes: &[u8],
    vfs_manifest_bytes: &[u8],
) -> Result<ValidatedRuntimeProviderSelection, ContainerError> {
    let policy: ProviderPolicy = serde_json::from_slice(policy_bytes).map_err(|error| {
        ContainerError::message(format!(
            "ASTRA_PROVIDER_POLICY_INVALID: provider policy v2 decode failed: {error}"
        ))
    })?;
    let registry: PluginExtensionRegistrySnapshot = serde_json::from_slice(registry_bytes)
        .map_err(|error| {
            ContainerError::message(format!(
                "ASTRA_PLUGIN_EXTENSION_REGISTRY_INVALID: registry v2 decode failed: {error}"
            ))
        })?;
    let target = registry
        .validate_embedded_package(&policy, package_id, profile)
        .map_err(|diagnostic| ContainerError::message(diagnostic.to_string()))?
        .to_string();

    let target_manifest: TargetManifest =
        serde_json::from_slice(target_manifest_bytes).map_err(|error| {
            ContainerError::message(format!(
                "ASTRA_TARGET_MANIFEST_INVALID: target manifest decode failed: {error}"
            ))
        })?;
    let validation = validate_manifest(&target_manifest, Some(&target));
    if validation.status == TargetValidationStatus::Blocked {
        let code = validation
            .diagnostics
            .first()
            .map(|diagnostic| diagnostic.code.as_str())
            .unwrap_or("ASTRA_TARGET_MANIFEST_INVALID");
        return Err(ContainerError::message(format!(
            "{code}: provider binding target is not a valid package target"
        )));
    }
    let target_descriptor = target_manifest
        .targets
        .iter()
        .find(|descriptor| descriptor.id == target)
        .ok_or_else(|| {
            ContainerError::message(
                "ASTRA_PLUGIN_BINDING_TARGET_MISSING: binding target is absent from target manifest",
            )
        })?;
    if target_descriptor.kind != TargetKind::Game || !target_descriptor.packaged {
        return Err(ContainerError::message(
            "ASTRA_PLUGIN_BINDING_TARGET_INELIGIBLE: binding target is not a packaged game target",
        ));
    }
    if target_descriptor.runtime_provider.as_deref()
        != Some(policy.runtime_provider.runtime_id.as_str())
    {
        return Err(ContainerError::message(
            "ASTRA_RUNTIME_PROVIDER_TARGET_MISMATCH: target and provider policy select different runtimes",
        ));
    }
    if !policy
        .runtime_provider
        .supported_targets
        .iter()
        .any(|supported| supported == &target || supported == "game")
    {
        return Err(ContainerError::message(
            "ASTRA_RUNTIME_PROVIDER_TARGET_UNSUPPORTED: runtime provider does not declare the selected target",
        ));
    }
    let vfs: VfsManifest = serde_json::from_slice(vfs_manifest_bytes).map_err(|error| {
        ContainerError::message(format!(
            "ASTRA_VFS_MANIFEST_INVALID: VFS manifest decode failed: {error}"
        ))
    })?;
    if let Some(diagnostic) = vfs.validate().into_iter().next() {
        return Err(ContainerError::message(format!(
            "{}: {}",
            diagnostic.code, diagnostic.message
        )));
    }
    for prefix in &vfs.prefixes {
        let binding = registry
            .bindings
            .iter()
            .find(|binding| {
                binding.slot == "vfs_provider" && binding.provider_id == prefix.provider_id
            })
            .ok_or_else(|| {
                ContainerError::message(format!(
                    "ASTRA_VFS_PROVIDER_MISSING: prefix {} has no explicit provider binding",
                    prefix.prefix
                ))
            })?;
        if binding.context.required_capability != prefix.backend.required_provider_capability() {
            return Err(ContainerError::message(format!(
                "ASTRA_VFS_PROVIDER_CAPABILITY_MISMATCH: prefix {} backend does not match its provider binding",
                prefix.prefix
            )));
        }
    }
    let mut resolved_uris = BTreeSet::new();
    for entry in &vfs.entries {
        let Some(layer) = vfs
            .layers
            .iter()
            .find(|layer| layer.layer_id == entry.layer_id)
        else {
            continue;
        };
        if (!layer.targets.is_empty() && !layer.targets.iter().any(|value| value == &target))
            || (!layer.profiles.is_empty() && !layer.profiles.iter().any(|value| value == profile))
            || !resolved_uris.insert(entry.uri.clone())
        {
            continue;
        }
        let prefix = vfs
            .prefixes
            .iter()
            .find(|prefix| prefix.prefix == entry.uri.prefix())
            .ok_or_else(|| {
                ContainerError::message(
                    "ASTRA_VFS_PREFIX_MISSING: VFS entry has no declared prefix",
                )
            })?;
        let capability = prefix.capabilities.first().ok_or_else(|| {
            ContainerError::message(
                "ASTRA_VFS_CAPABILITY_MISSING: VFS prefix has no read capability",
            )
        })?;
        vfs.resolve(
            &entry.uri,
            &ResolveContext {
                target: target.clone(),
                profile: profile.to_string(),
                capability: capability.clone(),
                provider_binding: prefix.provider_id.clone(),
            },
        )
        .map_err(|diagnostics| {
            let diagnostic = diagnostics
                .into_iter()
                .next()
                .expect("resolve error has diagnostic");
            ContainerError::message(format!("{}: {}", diagnostic.code, diagnostic.message))
        })?;
    }
    registry
        .resolve_embedded_runtime_provider(&policy, package_id, profile)
        .map_err(|diagnostic| ContainerError::message(diagnostic.to_string()))
}
