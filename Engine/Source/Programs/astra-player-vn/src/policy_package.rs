//! Preload only explicitly authorized package text. Runtime reads never access disk.
use crate::NativeVnHostError;
use astra_asset::{
    AssetCatalog, ResolveContext, VfsBackendKind, VfsManifest, VfsSourceRef, VfsUri,
};
use astra_core::Hash256;
use astra_package::PackageReader;
use astra_runtime::TaskScope;
use astra_vn_policy::{
    PolicyActionBinding, PolicyIoCapabilities, PolicyIoFailure, PolicyIoHost, PolicyReadText,
    POLICY_IO_MAX_BYTES,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    pin::Pin,
    sync::Arc,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedPolicyActions {
    pub(crate) package_hash: Hash256,
    pub(crate) bindings: Vec<PolicyActionBinding>,
    texts: BTreeMap<VfsUri, Arc<str>>,
    pub(crate) byte_len: u64,
}

impl PreparedPolicyActions {
    /// Synchronous bounded loading; cancellation is observed before and after each section.
    /// Callers running a platform event loop perform this on their owned startup worker.
    pub fn load(
        package: &PackageReader,
        max_bytes: u64,
        scope: &TaskScope,
    ) -> Result<Self, NativeVnHostError> {
        check_cancelled(scope)?;
        let bindings =
            astra_vn_package::load_policy_actions(package).map_err(|_| error("BINDINGS"))?;
        let mut result = Self {
            package_hash: package.package_hash(),
            bindings,
            texts: BTreeMap::new(),
            byte_len: 0,
        };
        if result.bindings.is_empty() {
            return Ok(result);
        }
        #[cfg(target_arch = "wasm32")]
        return Err(error("UNSUPPORTED"));
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut checked = BTreeSet::new();
            for binding in &result.bindings {
                if checked.insert((&binding.entry, &binding.source_hash)) {
                    let descriptors = astra_vn_policy::LuauPolicy::action_descriptors(
                        &binding.source,
                        Default::default(),
                    )
                    .map_err(|_| error("REGISTRATION"))?;
                    if descriptors.len()
                        != result
                            .bindings
                            .iter()
                            .filter(|other| other.entry == binding.entry)
                            .count()
                    {
                        return Err(error("REGISTRATION"));
                    }
                    for descriptor in descriptors {
                        if !result.bindings.iter().any(|other| {
                            other.entry == binding.entry
                                && other.source_hash == binding.source_hash
                                && other.descriptor == descriptor
                        }) {
                            return Err(error("REGISTRATION"));
                        }
                    }
                }
            }
            let resources: BTreeSet<_> = result
                .bindings
                .iter()
                .flat_map(|b| b.read_text.iter().cloned())
                .collect();
            PolicyIoCapabilities::read_text(resources.clone(), POLICY_IO_MAX_BYTES)
                .map_err(|_| error("GRANTS"))?;
            let manifest: VfsManifest = serde_json::from_slice(
                &package
                    .container()
                    .read_bounded("asset.vfs_manifest", 16 * 1024 * 1024)
                    .map_err(|_| error("MANIFEST"))?,
            )
            .map_err(|_| error("MANIFEST"))?;
            let catalog: AssetCatalog = serde_json::from_slice(
                &package
                    .container()
                    .read_bounded("asset.catalog", 16 * 1024 * 1024)
                    .map_err(|_| error("CATALOG"))?,
            )
            .map_err(|_| error("CATALOG"))?;
            let total = resources.len();
            for (index, resource) in resources.into_iter().enumerate() {
                check_cancelled(scope)?;
                // Authoring IDs use exactly the catalog mapping Cook emitted, never a URI guess.
                let matches: Vec<_> = catalog
                    .assets
                    .iter()
                    .filter(|asset| asset.asset_id == resource.as_str())
                    .collect();
                let [asset] = matches.as_slice() else {
                    return Err(error("ASSET_MAPPING"));
                };
                if resource.prefix() != "asset"
                    || asset.media_kind != "text"
                    || (!asset.profiles.is_empty()
                        && !asset
                            .profiles
                            .iter()
                            .any(|p| p == package.product_context().profile()))
                {
                    return Err(error("ASSET_TYPE_OR_PROFILE"));
                }
                let prefix = manifest
                    .prefixes
                    .iter()
                    .find(|prefix| prefix.prefix == asset.uri.prefix())
                    .ok_or_else(|| error("PREFIX"))?;
                if prefix.backend != VfsBackendKind::Package {
                    return Err(error("SOURCE"));
                }
                let entry = manifest
                    .resolve(
                        &asset.uri,
                        &ResolveContext {
                            target: package.product_context().target().into(),
                            profile: package.product_context().profile().into(),
                            capability: "vfs.backend.package".into(),
                            provider_binding: prefix.provider_id.clone(),
                        },
                    )
                    .map_err(|_| error("BINDING"))?
                    .ok_or_else(|| error("RESOURCE"))?;
                let VfsSourceRef::PackageSection { section_id } = &entry.source else {
                    return Err(error("SOURCE"));
                };
                let section = package
                    .container()
                    .section_entry(section_id)
                    .ok_or_else(|| error("SECTION"))?;
                if entry.media_kind != "text"
                    || entry.offset != 0
                    || entry.size != section.decoded_length
                    || entry.hash != section.hash
                    || entry.size > POLICY_IO_MAX_BYTES as u64
                    || result
                        .byte_len
                        .checked_add(entry.size)
                        .is_none_or(|size| size >= max_bytes)
                {
                    return Err(error("TEXT_BUDGET_OR_IDENTITY"));
                }
                let bytes = package
                    .container()
                    .read_bounded(section_id, POLICY_IO_MAX_BYTES)
                    .map_err(|_| error("READ"))?;
                check_cancelled(scope)?;
                if bytes.len() as u64 != entry.size || Hash256::from_sha256(&bytes) != entry.hash {
                    return Err(error("INTEGRITY"));
                }
                let text = String::from_utf8(bytes).map_err(|_| error("UTF8"))?;
                result.byte_len += text.len() as u64;
                result.texts.insert(resource, Arc::from(text));
                tracing::info!(
                    event = "player.policy.preload.progress",
                    completed = index + 1,
                    total,
                    byte_count = result.byte_len,
                    "Loaded authorized policy text"
                );
            }
            check_cancelled(scope)?;
            Ok(result)
        }
    }

    pub(crate) fn capabilities(&self) -> Result<PolicyIoCapabilities, NativeVnHostError> {
        PolicyIoCapabilities::read_text(self.texts.keys().cloned(), POLICY_IO_MAX_BYTES)
            .map_err(|_| error("GRANTS"))
    }
}
impl PolicyIoHost for PreparedPolicyActions {
    fn read_text(
        &self,
        request: PolicyReadText,
    ) -> Pin<Box<dyn Future<Output = Result<String, PolicyIoFailure>> + Send>> {
        let text = self.texts.get(&request.resource).cloned();
        Box::pin(async move {
            let text = text.ok_or(PolicyIoFailure::Host)?;
            if text.len() > request.max_bytes {
                return Err(PolicyIoFailure::Limit);
            }
            let mut bytes = Vec::with_capacity(text.len());
            for chunk in text.as_bytes().chunks(16 * 1024) {
                tokio::task::yield_now().await;
                bytes.extend_from_slice(chunk);
            }
            String::from_utf8(bytes).map_err(|_| PolicyIoFailure::Host)
        })
    }
}
fn check_cancelled(scope: &TaskScope) -> Result<(), NativeVnHostError> {
    if scope.is_cancelled() {
        Err(error("CANCELLED"))
    } else {
        Ok(())
    }
}
fn error(kind: &str) -> NativeVnHostError {
    NativeVnHostError::Package(format!("ASTRA_PLAYER_POLICY_{kind}"))
}
