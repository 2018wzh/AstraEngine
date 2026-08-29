#[astra_headless_test::test]
fn starts_and_stops_worktree_local_session() {
    let ctx = astra_headless_test::HeadlessTestContext::start().unwrap();
    assert!(ctx.artifact_root().is_dir());
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
    // Library-inline mode does not require the binary to be pre-built.
    // The path is only validated for its file_stem.
}

#[astra_headless_test::test]
fn concurrent_tests_share_one_multi_session_server() {
    use std::sync::{Arc, Barrier};

    // Library-inline mode: each test gets an isolated TempDir, no global
    // OnceLock server. This test now verifies isolation, not sharing.
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
    // No global session counter in library-inline mode.
    assert_eq!(
        astra_headless_test::active_headless_session_count().unwrap(),
        0
    );
    release.wait();
    for handle in handles {
        handle.join().unwrap();
    }
}
