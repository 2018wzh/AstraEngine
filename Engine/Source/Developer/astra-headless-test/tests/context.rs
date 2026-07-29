#[astra_headless_test::test]
fn starts_and_stops_worktree_local_session() {
    assert!(astra_headless_test::headless_build_identity_path()
        .unwrap()
        .is_file());
}

#[astra_headless_test::tokio_test]
async fn async_test_uses_same_per_binary_server() {
    tokio::task::yield_now().await;
}

#[astra_headless_test::test]
fn resolves_worktree_profile_binary_without_a_binary_environment_variable() {
    let binary = astra_headless_test::headless_binary_path().unwrap();
    assert!(binary.is_file());
    assert_eq!(
        binary.file_stem().and_then(|value| value.to_str()),
        Some("astra-headless")
    );
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
                let _context = astra_headless_test::HeadlessTestContext::start().unwrap();
                entered.wait();
                release.wait();
            })
        })
        .collect::<Vec<_>>();
    entered.wait();
    assert!(astra_headless_test::active_headless_session_count().unwrap() >= 5);
    release.wait();
    for handle in handles {
        handle.join().unwrap();
    }
}
