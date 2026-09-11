#[astra_headless_test::test]
fn starts_and_stops_worktree_local_session() {
    let ctx = astra_headless_test::HeadlessTestContext::start().unwrap();
    assert!(ctx.artifact_root().is_dir());
    let identity = astra_headless_test::headless_build_identity_path().unwrap();
    let identity: serde_json::Value =
        serde_json::from_slice(&std::fs::read(identity).unwrap()).unwrap();
    assert_eq!(identity["schema"], "astra.build_identity.v1");
    assert!(identity["identity_hash"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert!(astra_headless_test::active_headless_session_count().unwrap() >= 2);
}

#[astra_headless_test::tokio_test]
async fn async_test_uses_same_per_binary_server() {
    let ctx = astra_headless_test::HeadlessTestContext::start_async()
        .await
        .unwrap();
    assert!(ctx.artifact_root().is_dir());
    tokio::task::yield_now().await;
}

#[astra_headless_test::test]
fn resolves_worktree_profile_binary_without_a_binary_environment_variable() {
    let binary = astra_headless_test::headless_binary_path().unwrap();
    assert_eq!(
        binary.file_stem().and_then(|value| value.to_str()),
        Some("astra-headless")
    );
    assert!(binary.is_file());
}

#[astra_headless_test::test]
fn concurrent_tests_share_one_multi_session_server() {
    use std::sync::{Arc, Barrier};

    let entered = Arc::new(Barrier::new(5));
    let release = Arc::new(Barrier::new(5));
    let handles = (0..4)
        .map(|_| {
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            std::thread::spawn(move || {
                let ctx = astra_headless_test::HeadlessTestContext::start().unwrap();
                assert!(ctx.artifact_root().is_dir());
                entered.wait();
                release.wait();
            })
        })
        .collect::<Vec<_>>();
    let _ctx = astra_headless_test::HeadlessTestContext::start().unwrap();
    entered.wait();
    assert!(astra_headless_test::active_headless_session_count().unwrap() >= 6);
    release.wait();
    for handle in handles {
        handle.join().unwrap();
    }
}
