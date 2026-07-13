use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
};

use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{expected_cache_key, CookArtifact, CookError, CookProcessor, CookRequest};

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Default)]
pub struct CookCancellationToken(Arc<AtomicBool>);

impl CookCancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    pub fn check_cancelled(&self) -> Result<(), CookError> {
        if self.is_cancelled() {
            Err(CookError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookNode {
    pub request: CookRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookBatchRequest {
    pub nodes: Vec<CookNode>,
    pub max_concurrency: usize,
    pub limits: CookBatchLimits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CookBatchLimits {
    pub max_node_count: usize,
    pub max_source_bytes_per_node: u64,
    pub max_total_source_bytes: u64,
    pub max_concurrency: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookBatchResult {
    pub schema: String,
    pub graph_hash: Hash256,
    pub artifacts: Vec<CookArtifact>,
    pub cache_hit_count: u64,
    pub cooked_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CookBatchManifest {
    pub schema: String,
    pub graph_hash: Hash256,
    pub cache_hit_count: u64,
    pub cooked_count: u64,
    pub artifacts: Vec<CookBatchArtifactRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CookBatchArtifactRecord {
    pub asset_id: String,
    pub section_id: String,
    pub cache_key: Hash256,
    pub payload_hash: Hash256,
    pub byte_size: u64,
    pub file: String,
}

#[derive(Clone, Default)]
pub struct CookProcessorRegistry {
    processors: BTreeMap<String, Arc<dyn CookProcessor>>,
}

impl CookProcessorRegistry {
    pub fn register<P>(&mut self, processor: P) -> Result<(), CookError>
    where
        P: CookProcessor + 'static,
    {
        let id = processor.processor_id().to_string();
        if self
            .processors
            .insert(id.clone(), Arc::new(processor))
            .is_some()
        {
            return Err(CookError::message(format!(
                "ASTRA_COOK_PROCESSOR_DUPLICATE: processor {id} is registered more than once"
            )));
        }
        Ok(())
    }

    fn get(&self, id: &str) -> Result<&dyn CookProcessor, CookError> {
        self.processors.get(id).map(Arc::as_ref).ok_or_else(|| {
            CookError::message(format!(
                "ASTRA_COOK_PROCESSOR_MISSING: processor {id} is not registered"
            ))
        })
    }
}

#[derive(Debug, Clone)]
pub struct FileCookCache {
    root: PathBuf,
}

impl FileCookCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn load(&self, key: &Hash256) -> Result<Option<CookArtifact>, CookError> {
        let path = self.path_for(key);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(CookError::message(format!(
                    "ASTRA_COOK_CACHE_READ: {error}"
                )))
            }
        };
        let artifact = postcard::from_bytes::<CookArtifact>(&bytes)
            .map_err(|error| CookError::message(format!("ASTRA_COOK_CACHE_CORRUPT: {error}")))?;
        if artifact.cache_key != *key
            || artifact.payload_hash != Hash256::from_sha256(&artifact.payload)
        {
            return Err(CookError::message(
                "ASTRA_COOK_CACHE_CORRUPT: cached artifact identity mismatch",
            ));
        }
        Ok(Some(artifact))
    }

    pub fn store(&self, artifact: &CookArtifact) -> Result<(), CookError> {
        fs::create_dir_all(&self.root)
            .map_err(|error| CookError::message(format!("ASTRA_COOK_CACHE_CREATE: {error}")))?;
        let destination = self.path_for(&artifact.cache_key);
        if let Some(existing) = self.load(&artifact.cache_key)? {
            if existing != *artifact {
                return Err(CookError::message(
                    "ASTRA_COOK_CACHE_CONFLICT: cache key resolves to different artifact bytes",
                ));
            }
            return Ok(());
        }
        let encoded = postcard::to_allocvec(artifact)
            .map_err(|error| CookError::message(format!("ASTRA_COOK_CACHE_ENCODE: {error}")))?;
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = self.root.join(format!(
            ".{}.{}.{}.tmp",
            artifact.cache_key.to_hex(),
            std::process::id(),
            sequence
        ));
        let write_result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)?;
            file.write_all(&encoded)?;
            file.sync_all()?;
            fs::hard_link(&temporary, &destination)?;
            fs::remove_file(&temporary)
        })();
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temporary);
            if destination.is_file() {
                let existing = self.load(&artifact.cache_key)?.ok_or_else(|| {
                    CookError::message("ASTRA_COOK_CACHE_COMMIT: destination disappeared")
                })?;
                if existing == *artifact {
                    return Ok(());
                }
            }
            return Err(CookError::message(format!(
                "ASTRA_COOK_CACHE_COMMIT: {error}"
            )));
        }
        Ok(())
    }

    fn path_for(&self, key: &Hash256) -> PathBuf {
        self.root.join(format!("{}.cook", key.to_hex()))
    }
}

pub struct CookBatchExecutor<'a> {
    registry: &'a CookProcessorRegistry,
    cache: Option<&'a FileCookCache>,
}

impl<'a> CookBatchExecutor<'a> {
    pub fn new(registry: &'a CookProcessorRegistry, cache: Option<&'a FileCookCache>) -> Self {
        Self { registry, cache }
    }

    pub fn execute(
        &self,
        request: CookBatchRequest,
        cancellation: &CookCancellationToken,
    ) -> Result<CookBatchResult, CookError> {
        cancellation.check_cancelled()?;
        if request.max_concurrency == 0 {
            return Err(CookError::message(
                "ASTRA_COOK_CONCURRENCY_INVALID: max_concurrency must be positive",
            ));
        }
        validate_limits(&request)?;
        let (levels, graph_hash) = validate_graph(&request.nodes)?;
        let nodes = request
            .nodes
            .into_iter()
            .map(|node| (node.request.sidecar.id.to_string(), node))
            .collect::<BTreeMap<_, _>>();
        let mut artifacts = Vec::new();
        let mut artifact_hashes = BTreeMap::new();
        let mut cache_hit_count = 0_u64;
        let mut cooked_count = 0_u64;
        for level in levels {
            cancellation.check_cancelled()?;
            for chunk in level.chunks(request.max_concurrency) {
                let resolved_nodes = chunk
                    .iter()
                    .map(|asset_id| {
                        let mut node = nodes.get(asset_id).expect("validated graph node").clone();
                        node.request.dependency_artifacts = node
                            .request
                            .sidecar
                            .dependencies
                            .iter()
                            .map(|dependency| {
                                let dependency_id = dependency.to_string();
                                let hash = artifact_hashes.get(&dependency_id).copied().ok_or_else(
                                    || {
                                        CookError::message(format!(
                                            "ASTRA_COOK_DEPENDENCY_RESULT_MISSING: {asset_id} dependency {dependency_id} has no cooked artifact"
                                        ))
                                    },
                                )?;
                                Ok((dependency_id, hash))
                            })
                            .collect::<Result<BTreeMap<_, _>, CookError>>()?;
                        Ok((asset_id.clone(), node))
                    })
                    .collect::<Result<Vec<_>, CookError>>()?;
                let results = Mutex::new(Vec::with_capacity(chunk.len()));
                thread::scope(|scope| {
                    for (asset_id, node) in &resolved_nodes {
                        let results = &results;
                        scope.spawn(move || {
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    self.execute_node(node, cancellation)
                                }))
                                .map_err(|_| {
                                    CookError::message(
                                        "ASTRA_COOK_WORKER_PANIC: cook processor panicked",
                                    )
                                })
                                .and_then(|result| result);
                            results
                                .lock()
                                .expect("cook result lock poisoned")
                                .push((asset_id.clone(), result));
                        });
                    }
                });
                let mut results = results.into_inner().map_err(|_| {
                    CookError::message("ASTRA_COOK_WORKER_PANIC: result lock poisoned")
                })?;
                results.sort_by(|left, right| left.0.cmp(&right.0));
                for (_, result) in results {
                    let (artifact, cache_hit) = result?;
                    if cache_hit {
                        cache_hit_count += 1;
                    } else {
                        cooked_count += 1;
                    }
                    artifact_hashes.insert(artifact.asset_id.clone(), artifact.payload_hash);
                    artifacts.push(artifact);
                }
            }
        }
        cancellation.check_cancelled()?;
        artifacts.sort_by(|left, right| left.asset_id.cmp(&right.asset_id));
        Ok(CookBatchResult {
            schema: "astra.cook_batch_result.v1".to_string(),
            graph_hash,
            artifacts,
            cache_hit_count,
            cooked_count,
        })
    }

    fn execute_node(
        &self,
        node: &CookNode,
        cancellation: &CookCancellationToken,
    ) -> Result<(CookArtifact, bool), CookError> {
        cancellation.check_cancelled()?;
        let processor = self.registry.get(&node.request.sidecar.cook.processor)?;
        let expected_key = expected_cache_key(&node.request, processor.processor_id())?;
        if let Some(cache) = self.cache {
            if let Some(artifact) = cache.load(&expected_key)? {
                artifact.validate_for(
                    &node.request,
                    processor.processor_id(),
                    processor.processor_version(),
                )?;
                cancellation.check_cancelled()?;
                return Ok((artifact, true));
            }
        }
        let artifact = processor.cook(node.request.clone())?;
        cancellation.check_cancelled()?;
        if let Some(cache) = self.cache {
            cache.store(&artifact)?;
        }
        Ok((artifact, false))
    }
}

fn validate_limits(request: &CookBatchRequest) -> Result<(), CookError> {
    if request.limits.max_node_count == 0
        || request.limits.max_source_bytes_per_node == 0
        || request.limits.max_total_source_bytes == 0
        || request.limits.max_concurrency == 0
    {
        return Err(CookError::message(
            "ASTRA_COOK_LIMIT_INVALID: every cook batch limit must be positive",
        ));
    }
    if request.nodes.len() > request.limits.max_node_count {
        return Err(CookError::message(
            "ASTRA_COOK_NODE_LIMIT: cook graph exceeds the configured node limit",
        ));
    }
    if request.max_concurrency > request.limits.max_concurrency {
        return Err(CookError::message(
            "ASTRA_COOK_CONCURRENCY_LIMIT: requested concurrency exceeds the configured limit",
        ));
    }
    let mut total = 0_u64;
    for node in &request.nodes {
        let bytes = node.request.source_bytes.len() as u64;
        if bytes > request.limits.max_source_bytes_per_node {
            return Err(CookError::message(format!(
                "ASTRA_COOK_ASSET_SIZE_LIMIT: asset {} exceeds the configured source byte limit",
                node.request.sidecar.id
            )));
        }
        total = total.checked_add(bytes).ok_or_else(|| {
            CookError::message("ASTRA_COOK_TOTAL_SIZE_OVERFLOW: source byte total overflowed")
        })?;
        if total > request.limits.max_total_source_bytes {
            return Err(CookError::message(
                "ASTRA_COOK_TOTAL_SIZE_LIMIT: cook batch exceeds the configured source byte limit",
            ));
        }
    }
    Ok(())
}

fn validate_graph(nodes: &[CookNode]) -> Result<(Vec<Vec<String>>, Hash256), CookError> {
    if nodes.is_empty() {
        return Ok((
            Vec::new(),
            Hash256::from_sha256(b"astra.cook_graph.v1|empty"),
        ));
    }
    let mut by_id = BTreeMap::new();
    for node in nodes {
        let id = node.request.sidecar.id.to_string();
        if by_id.insert(id.clone(), node).is_some() {
            return Err(CookError::message(format!(
                "ASTRA_COOK_GRAPH_DUPLICATE: asset {id} appears more than once"
            )));
        }
    }
    for (id, node) in &by_id {
        let mut dependencies = BTreeSet::new();
        for dependency in &node.request.sidecar.dependencies {
            let dependency = dependency.to_string();
            if &dependency == id {
                return Err(CookError::message(format!(
                    "ASTRA_COOK_GRAPH_SELF_DEPENDENCY: asset {id} depends on itself"
                )));
            }
            if !by_id.contains_key(&dependency) {
                return Err(CookError::message(format!(
                    "ASTRA_COOK_GRAPH_MISSING_DEPENDENCY: asset {id} depends on missing {dependency}"
                )));
            }
            if !dependencies.insert(dependency.clone()) {
                return Err(CookError::message(format!(
                    "ASTRA_COOK_GRAPH_DUPLICATE_DEPENDENCY: asset {id} repeats {dependency}"
                )));
            }
        }
    }
    let mut remaining = by_id
        .iter()
        .map(|(id, node)| {
            (
                id.clone(),
                node.request
                    .sidecar
                    .dependencies
                    .iter()
                    .map(ToString::to_string)
                    .collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut resolved = BTreeSet::new();
    let mut levels = Vec::new();
    while !remaining.is_empty() {
        let level = remaining
            .iter()
            .filter(|(_, dependencies)| dependencies.is_subset(&resolved))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        if level.is_empty() {
            return Err(CookError::message(
                "ASTRA_COOK_GRAPH_CYCLE: asset dependency graph contains a cycle",
            ));
        }
        for id in &level {
            remaining.remove(id);
            resolved.insert(id.clone());
        }
        levels.push(level);
    }
    let identity = by_id
        .iter()
        .map(|(id, node)| {
            let mut dependencies = node
                .request
                .sidecar
                .dependencies
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            dependencies.sort();
            let cache_key =
                expected_cache_key(&node.request, &node.request.sidecar.cook.processor)?;
            Ok(format!(
                "{id}|{}|{}|{cache_key}",
                node.request.target_profile,
                dependencies.join(",")
            ))
        })
        .collect::<Result<Vec<_>, CookError>>()?
        .join("\n");
    Ok((levels, Hash256::from_sha256(identity.as_bytes())))
}

pub fn commit_batch_directory(
    destination: &Path,
    result: &CookBatchResult,
) -> Result<(), CookError> {
    let parent = destination.parent().ok_or_else(|| {
        CookError::message("ASTRA_COOK_COMMIT_PATH: destination has no parent directory")
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| CookError::message(format!("ASTRA_COOK_COMMIT_CREATE: {error}")))?;
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let staging = parent.join(format!(
        ".astra-cook-stage-{}-{sequence}",
        std::process::id()
    ));
    if staging.exists() {
        return Err(CookError::message(
            "ASTRA_COOK_COMMIT_CONFLICT: staging directory already exists",
        ));
    }
    fs::create_dir(&staging)
        .map_err(|error| CookError::message(format!("ASTRA_COOK_COMMIT_CREATE: {error}")))?;
    let write_result = (|| {
        for artifact in &result.artifacts {
            fs::write(
                staging.join(format!("{}.bin", artifact.cache_key.to_hex())),
                &artifact.payload,
            )?;
        }
        let manifest = CookBatchManifest {
            schema: "astra.cook_batch_manifest.v1".to_string(),
            graph_hash: result.graph_hash,
            cache_hit_count: result.cache_hit_count,
            cooked_count: result.cooked_count,
            artifacts: result
                .artifacts
                .iter()
                .map(|artifact| CookBatchArtifactRecord {
                    asset_id: artifact.asset_id.clone(),
                    section_id: artifact.section_id.clone(),
                    cache_key: artifact.cache_key,
                    payload_hash: artifact.payload_hash,
                    byte_size: artifact.payload.len() as u64,
                    file: format!("{}.bin", artifact.cache_key.to_hex()),
                })
                .collect(),
        };
        fs::write(
            staging.join("cook_batch.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_dir_all(&staging);
        return Err(CookError::message(format!(
            "ASTRA_COOK_COMMIT_WRITE: {error}"
        )));
    }
    if destination.exists() {
        let backup = parent.join(format!(
            ".astra-cook-backup-{}-{sequence}",
            std::process::id()
        ));
        fs::rename(destination, &backup)
            .map_err(|error| CookError::message(format!("ASTRA_COOK_COMMIT_BACKUP: {error}")))?;
        if let Err(error) = fs::rename(&staging, destination) {
            let _ = fs::rename(&backup, destination);
            let _ = fs::remove_dir_all(&staging);
            return Err(CookError::message(format!(
                "ASTRA_COOK_COMMIT_SWAP: {error}"
            )));
        }
        if let Err(error) = fs::remove_dir_all(&backup) {
            let failed_output = parent.join(format!(
                ".astra-cook-rollback-{}-{sequence}",
                std::process::id()
            ));
            let rollback = fs::rename(destination, &failed_output)
                .and_then(|_| fs::rename(&backup, destination));
            let _ = fs::remove_dir_all(&failed_output);
            return match rollback {
                Ok(()) => Err(CookError::message(format!(
                    "ASTRA_COOK_COMMIT_CLEANUP: {error}; previous output restored"
                ))),
                Err(rollback_error) => Err(CookError::message(format!(
                    "ASTRA_COOK_COMMIT_ROLLBACK: cleanup failed ({error}) and rollback failed ({rollback_error})"
                ))),
            };
        }
    } else {
        fs::rename(&staging, destination)
            .map_err(|error| CookError::message(format!("ASTRA_COOK_COMMIT_SWAP: {error}")))?;
    }
    Ok(())
}
