#![cfg(not(target_arch = "wasm32"))]

use astra_platform::{PackageSourcePolicy, PlatformErrorCode};
use astra_platform_general::HttpRangeClient;

#[test]
fn https_client_requires_an_explicit_allowlist_policy() {
    let error = HttpRangeClient::from_policies(&[]).err().unwrap();
    assert_eq!(error.code, PlatformErrorCode::PermissionDenied);
}

#[test]
fn https_client_rejects_unapproved_origin_before_network_access() {
    let client = HttpRangeClient::from_policies(&[PackageSourcePolicy::HttpsRange {
        allowed_origins: vec!["https://cdn.example.test".to_string()],
    }])
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut cache = astra_platform_general::VerifiedPackageCache::open(
        temp.path(),
        astra_platform::PackageCachePolicy {
            max_entry_bytes: 64,
            max_total_bytes: 128,
        },
    )
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    assert_eq!(
        runtime
            .block_on(client.fetch_into_cache(
                "https://other.example.test/game.astrapkg",
                "sha256:0000000000000000000000000000000000000000000000000000000000000000",
                &mut cache
            ))
            .unwrap_err()
            .code,
        PlatformErrorCode::PermissionDenied
    );
}
