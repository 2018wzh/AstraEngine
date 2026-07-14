#![cfg(all(target_os = "windows", feature = "platform-test-driver"))]

use astra_platform::{PackageSourceRequest, PlatformHostFactory, PlatformHostProfile};

#[tokio::test]
async fn windows_host_uses_atomic_saved_games_store_and_hash_bound_bundle_source() {
    let temp = tempfile::tempdir().unwrap();
    let save_root = temp.path().join("SavedGames");
    let bundle_root = temp.path().join("Bundle");
    std::fs::create_dir_all(bundle_root.join("package")).unwrap();
    let package_bytes = [0, 1, 2, 3, 4, 5];
    std::fs::write(bundle_root.join("package/game.astrapkg"), package_bytes).unwrap();
    let package_hash = astra_core::Hash256::from_sha256(&package_bytes).to_string();

    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.test");
    let session = astra_platform_windows::factory_with_test_roots(&save_root, &bundle_root)
        .start(astra_platform::HostLaunchProfile::platform(profile))
        .await
        .expect("start host with isolated roots");

    let transaction = session.client.begin_save("slot-1").await.unwrap();
    session
        .client
        .write_save(transaction, vec![9, 8, 7])
        .await
        .unwrap();
    let save_hash = session.client.commit_save(transaction).await.unwrap();
    assert!(save_hash.starts_with("sha256:"));
    assert_eq!(session.client.read_save("slot-1").await.unwrap(), [9, 8, 7]);

    let source = session
        .client
        .open_package(PackageSourceRequest::Bundled {
            relative_path: "package/game.astrapkg".to_string(),
            expected_hash: package_hash,
        })
        .await
        .unwrap();
    assert_eq!(
        session
            .client
            .read_package_range(source, 2, 3)
            .await
            .unwrap(),
        [2, 3, 4]
    );
    session.client.close_package(source).await.unwrap();
    session.client.shutdown().await.unwrap();
}
