use super::*;

fn job(session_id: &str, root: &str) -> BatchJob {
    BatchJob {
        session_id: session_id.to_string(),
        kind: BatchJobKind::Route,
        gpu: false,
        profile: "profile.json".into(),
        package: "game.astrapak".into(),
        input: "input.jsonl".into(),
        artifact_root: format!("{root}/concurrent").into(),
        serial_artifact_root: format!("{root}/serial").into(),
        checkpoint_config: None,
        build_identity: "build-identity.json".into(),
        performance: None,
        timeout_ms: 1_000,
    }
}

#[test]
fn batch_manifest_requires_unique_session_and_artifact_identity() {
    let mut manifest = BatchManifest {
        schema: MANIFEST_SCHEMA.to_string(),
        worker_limit: 2,
        jobs: vec![job("route.a", "a"), job("route.b", "b")],
    };
    validate_manifest(&manifest).unwrap();

    manifest.jobs[1].session_id = "route.a".into();
    assert!(validate_manifest(&manifest)
        .unwrap_err()
        .starts_with("ASTRA_HEADLESS_BATCH_SESSION_DUPLICATE"));
    manifest.jobs[1].session_id = "route.b".into();
    manifest.jobs[1].artifact_root = manifest.jobs[0].artifact_root.clone();
    assert!(validate_manifest(&manifest)
        .unwrap_err()
        .starts_with("ASTRA_HEADLESS_BATCH_ARTIFACT_ROOT"));
}

#[test]
fn batch_manifest_bounds_global_workers_and_child_timeout() {
    let mut manifest = BatchManifest {
        schema: MANIFEST_SCHEMA.to_string(),
        worker_limit: 9,
        jobs: vec![job("route.a", "a")],
    };
    assert!(validate_manifest(&manifest)
        .unwrap_err()
        .starts_with("ASTRA_HEADLESS_BATCH_WORKER_LIMIT"));
    manifest.worker_limit = 1;
    manifest.jobs[0].timeout_ms = 99;
    assert!(validate_manifest(&manifest)
        .unwrap_err()
        .starts_with("ASTRA_HEADLESS_BATCH_TIMEOUT"));
}

#[test]
fn batch_selects_concurrency_from_cap_jobs_and_hardware() {
    assert_eq!(select_concurrency(8, 12, 6).unwrap(), 6);
    assert_eq!(select_concurrency(8, 3, 16).unwrap(), 3);
    assert_eq!(select_concurrency(2, 12, 16).unwrap(), 2);
    assert!(select_concurrency(8, 0, 16)
        .unwrap_err()
        .starts_with("ASTRA_HEADLESS_BATCH_EMPTY"));
    assert!(select_concurrency(8, 1, 0)
        .unwrap_err()
        .starts_with("ASTRA_HEADLESS_BATCH_PARALLELISM_QUERY"));
}

#[test]
fn batch_partitions_internal_workers_without_oversubscription() {
    assert_eq!(concurrent_session_worker_limit(8, 1).unwrap(), 8);
    assert_eq!(concurrent_session_worker_limit(8, 2).unwrap(), 4);
    assert_eq!(concurrent_session_worker_limit(8, 3).unwrap(), 2);
    assert_eq!(concurrent_session_worker_limit(8, 8).unwrap(), 1);
    assert!(concurrent_session_worker_limit(8, 0)
        .unwrap_err()
        .starts_with("ASTRA_HEADLESS_BATCH_SELECTED_CONCURRENCY"));
    assert!(concurrent_session_worker_limit(4, 5)
        .unwrap_err()
        .starts_with("ASTRA_HEADLESS_BATCH_SELECTED_CONCURRENCY"));
}

#[test]
fn batch_utilization_is_capacity_normalized_and_bounded() {
    assert_eq!(utilization_permille(4_000, 1_000, 4), 1_000);
    assert_eq!(utilization_permille(2_000, 1_000, 4), 500);
    assert_eq!(utilization_permille(10_000, 1_000, 4), 1_000);
    assert_eq!(utilization_permille(1, 0, 4), 0);
}
