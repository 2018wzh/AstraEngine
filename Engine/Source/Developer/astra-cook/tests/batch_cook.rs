use std::{
    fs,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use astra_asset::{AssetSidecar, CookSettings, ReviewStatus};
use astra_cook::{
    commit_batch_directory, CookArtifact, CookBatchExecutor, CookBatchLimits, CookBatchRequest,
    CookCancellationToken, CookError, CookNode, CookProcessor, CookProcessorRegistry, CookRequest,
    DefaultCookProcessor, FileCookCache,
};
use astra_core::Hash256;

fn node(id: &str, source: &[u8], dependencies: &[&str]) -> CookNode {
    let source_hash = Hash256::from_sha256(source);
    CookNode {
        request: CookRequest {
            sidecar: AssetSidecar {
                schema: "astra.asset.v1".to_string(),
                id: format!("asset:/{id}").parse().unwrap(),
                source: format!("content/{id}.bin"),
                source_hash: Some(source_hash),
                asset_type: "binary.test".to_string(),
                license: Some("test-fixture".to_string()),
                importer: "astra.import.test".to_string(),
                font: None,
                dependencies: dependencies
                    .iter()
                    .map(|dependency| dependency.parse().unwrap())
                    .collect(),
                cook: CookSettings {
                    processor: "astra.cook.test".to_string(),
                    target_profiles: vec!["test".to_string()],
                    params: Default::default(),
                },
                review: ReviewStatus::Accepted,
            },
            source_bytes: source.to_vec(),
            target_profile: "test".to_string(),
            processor_version: "1.0.0".to_string(),
            dependency_artifacts: Default::default(),
        },
    }
}

fn registry() -> CookProcessorRegistry {
    let mut registry = CookProcessorRegistry::default();
    registry
        .register(DefaultCookProcessor::new("astra.cook.test", "1.0.0"))
        .unwrap();
    registry
}

fn limits() -> CookBatchLimits {
    CookBatchLimits {
        max_node_count: 512,
        max_source_bytes_per_node: 16 * 1024 * 1024,
        max_total_source_bytes: 32 * 1024 * 1024,
        max_concurrency: 8,
    }
}

struct SlowProcessor {
    started: Arc<AtomicBool>,
    inner: DefaultCookProcessor,
}

impl CookProcessor for SlowProcessor {
    fn processor_id(&self) -> &str {
        self.inner.processor_id()
    }

    fn processor_version(&self) -> &str {
        self.inner.processor_version()
    }

    fn cook(&self, request: CookRequest) -> Result<CookArtifact, CookError> {
        self.started.store(true, Ordering::Release);
        std::thread::sleep(Duration::from_millis(50));
        self.inner.cook(request)
    }
}

struct PanicProcessor;

impl CookProcessor for PanicProcessor {
    fn processor_id(&self) -> &str {
        "astra.cook.panic"
    }

    fn processor_version(&self) -> &str {
        "1.0.0"
    }

    fn cook(&self, _request: CookRequest) -> Result<CookArtifact, CookError> {
        panic!("intentional processor panic")
    }
}

#[test]
fn batch_cook_validates_graph_uses_content_cache_and_commits_atomically() {
    let temp = tempfile::tempdir().unwrap();
    let cache = FileCookCache::new(temp.path().join("cache"));
    let registry = registry();
    let executor = CookBatchExecutor::new(&registry, Some(&cache));
    let request = || CookBatchRequest {
        nodes: vec![
            node("textures/background", b"background", &[]),
            node(
                "materials/background",
                b"material",
                &["asset:/textures/background"],
            ),
        ],
        max_concurrency: 2,
        limits: limits(),
    };
    let cancellation = CookCancellationToken::default();
    let first = executor.execute(request(), &cancellation).unwrap();
    assert_eq!(first.cooked_count, 2);
    assert_eq!(first.cache_hit_count, 0);
    assert_eq!(
        first
            .artifacts
            .iter()
            .map(|artifact| artifact.asset_id.as_str())
            .collect::<Vec<_>>(),
        ["asset:/materials/background", "asset:/textures/background"]
    );

    let second = executor.execute(request(), &cancellation).unwrap();
    assert_eq!(second.cooked_count, 0);
    assert_eq!(second.cache_hit_count, 2);
    assert_eq!(first.graph_hash, second.graph_hash);

    let changed = executor
        .execute(
            CookBatchRequest {
                nodes: vec![
                    node("textures/background", b"background-v2", &[]),
                    node(
                        "materials/background",
                        b"material",
                        &["asset:/textures/background"],
                    ),
                ],
                max_concurrency: 2,
                limits: limits(),
            },
            &cancellation,
        )
        .unwrap();
    assert_eq!(changed.cooked_count, 2);
    assert_ne!(changed.graph_hash, first.graph_hash);

    let destination = temp.path().join("published");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("stale.bin"), b"stale").unwrap();
    commit_batch_directory(&destination, &second).unwrap();
    assert!(!destination.join("stale.bin").exists());
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(destination.join("cook_batch.json")).unwrap()).unwrap();
    assert_eq!(manifest["schema"], "astra.cook_batch_manifest.v1");
    assert_eq!(manifest["artifacts"].as_array().unwrap().len(), 2);
    assert!(manifest["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .all(|artifact| artifact.get("payload").is_none()));
}

#[test]
fn batch_cook_rejects_graph_errors_cancellation_and_corrupt_cache() {
    let temp = tempfile::tempdir().unwrap();
    let cache_root = temp.path().join("cache");
    let cache = FileCookCache::new(&cache_root);
    let registry = registry();
    let executor = CookBatchExecutor::new(&registry, Some(&cache));

    for nodes in [
        vec![node("a", b"a", &["asset:/missing"])],
        vec![
            node("a", b"a", &["asset:/b"]),
            node("b", b"b", &["asset:/a"]),
        ],
        vec![node("a", b"a", &[]), node("a", b"other", &[])],
    ] {
        assert!(executor
            .execute(
                CookBatchRequest {
                    nodes,
                    max_concurrency: 2,
                    limits: limits(),
                },
                &CookCancellationToken::default(),
            )
            .is_err());
    }

    let cancelled = CookCancellationToken::default();
    cancelled.cancel();
    assert!(executor
        .execute(
            CookBatchRequest {
                nodes: vec![node("cancelled", b"cancelled", &[])],
                max_concurrency: 1,
                limits: limits(),
            },
            &cancelled,
        )
        .unwrap_err()
        .to_string()
        .contains("ASTRA_COOK_CANCELLED"));

    let request = CookBatchRequest {
        nodes: vec![node("cached", b"cached", &[])],
        max_concurrency: 1,
        limits: limits(),
    };
    let result = executor
        .execute(request.clone(), &CookCancellationToken::default())
        .unwrap();
    let cache_file = cache_root.join(format!("{}.cook", result.artifacts[0].cache_key.to_hex()));
    fs::write(&cache_file, b"corrupt").unwrap();
    assert!(executor
        .execute(request, &CookCancellationToken::default())
        .unwrap_err()
        .to_string()
        .contains("ASTRA_COOK_CACHE_CORRUPT"));

    fs::remove_file(&cache_file).unwrap();
    let request = CookBatchRequest {
        nodes: vec![node("cached", b"cached", &[])],
        max_concurrency: 1,
        limits: limits(),
    };
    executor
        .execute(request.clone(), &CookCancellationToken::default())
        .unwrap();
    let mut semantic_drift: CookArtifact =
        postcard::from_bytes(&fs::read(&cache_file).unwrap()).unwrap();
    semantic_drift.target_profile = "different".to_string();
    fs::write(&cache_file, postcard::to_allocvec(&semantic_drift).unwrap()).unwrap();
    assert!(executor
        .execute(request, &CookCancellationToken::default())
        .unwrap_err()
        .to_string()
        .contains("complete cook identity"));
}

#[test]
fn batch_cook_rejects_zero_concurrency_and_processor_version_drift() {
    let registry = registry();
    let executor = CookBatchExecutor::new(&registry, None);
    assert!(executor
        .execute(
            CookBatchRequest {
                nodes: vec![node("zero", b"zero", &[])],
                max_concurrency: 0,
                limits: limits(),
            },
            &CookCancellationToken::default(),
        )
        .is_err());
    let mut drift = node("drift", b"drift", &[]);
    drift.request.processor_version = "2.0.0".to_string();
    let error = executor
        .execute(
            CookBatchRequest {
                nodes: vec![drift],
                max_concurrency: 1,
                limits: limits(),
            },
            &CookCancellationToken::default(),
        )
        .unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_COOK_PROCESSOR_VERSION_MISMATCH"));

    for constrained in [
        CookBatchLimits {
            max_node_count: 1,
            ..limits()
        },
        CookBatchLimits {
            max_source_bytes_per_node: 2,
            ..limits()
        },
        CookBatchLimits {
            max_total_source_bytes: 2,
            ..limits()
        },
        CookBatchLimits {
            max_concurrency: 1,
            ..limits()
        },
    ] {
        let nodes = if constrained.max_node_count == 1 {
            vec![node("limit-a", b"data", &[]), node("limit-b", b"data", &[])]
        } else {
            vec![node("limit", b"data", &[])]
        };
        assert!(executor
            .execute(
                CookBatchRequest {
                    nodes,
                    max_concurrency: 2,
                    limits: constrained,
                },
                &CookCancellationToken::default(),
            )
            .is_err());
    }
}

#[test]
fn batch_cook_cancels_in_flight_and_contains_processor_panics() {
    let started = Arc::new(AtomicBool::new(false));
    let mut registry = CookProcessorRegistry::default();
    registry
        .register(SlowProcessor {
            started: started.clone(),
            inner: DefaultCookProcessor::new("astra.cook.slow", "1.0.0"),
        })
        .unwrap();
    let mut slow_node = node("slow", b"slow", &[]);
    slow_node.request.sidecar.cook.processor = "astra.cook.slow".to_string();
    let cancellation = CookCancellationToken::default();
    let executor = CookBatchExecutor::new(&registry, None);
    std::thread::scope(|scope| {
        let handle = scope.spawn(|| {
            executor.execute(
                CookBatchRequest {
                    nodes: vec![slow_node],
                    max_concurrency: 1,
                    limits: limits(),
                },
                &cancellation,
            )
        });
        while !started.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        cancellation.cancel();
        assert!(handle
            .join()
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("ASTRA_COOK_CANCELLED"));
    });

    let mut panic_registry = CookProcessorRegistry::default();
    panic_registry.register(PanicProcessor).unwrap();
    let mut panic_node = node("panic", b"panic", &[]);
    panic_node.request.sidecar.cook.processor = "astra.cook.panic".to_string();
    let error = CookBatchExecutor::new(&panic_registry, None)
        .execute(
            CookBatchRequest {
                nodes: vec![panic_node],
                max_concurrency: 1,
                limits: limits(),
            },
            &CookCancellationToken::default(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("ASTRA_COOK_WORKER_PANIC"));
}

#[test]
fn batch_cook_handles_many_nodes_and_large_payload_with_stable_incremental_identity() {
    let temp = tempfile::tempdir().unwrap();
    let cache = FileCookCache::new(temp.path().join("cache"));
    let registry = registry();
    let executor = CookBatchExecutor::new(&registry, Some(&cache));
    let request = || {
        let mut nodes = (0..128)
            .map(|index| {
                node(
                    &format!("bulk/{index:03}"),
                    format!("payload-{index:03}").as_bytes(),
                    &[],
                )
            })
            .collect::<Vec<_>>();
        nodes.push(node("bulk/large", &vec![0x5a; 8 * 1024 * 1024], &[]));
        CookBatchRequest {
            nodes,
            max_concurrency: 4,
            limits: limits(),
        }
    };
    let first = executor
        .execute(request(), &CookCancellationToken::default())
        .unwrap();
    assert_eq!(first.artifacts.len(), 129);
    assert_eq!(first.cooked_count, 129);
    let second = executor
        .execute(request(), &CookCancellationToken::default())
        .unwrap();
    assert_eq!(second.cache_hit_count, 129);
    assert_eq!(second.graph_hash, first.graph_hash);
}
