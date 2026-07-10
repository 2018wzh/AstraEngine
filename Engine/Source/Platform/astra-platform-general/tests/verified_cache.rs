#![cfg(not(target_arch = "wasm32"))]

use astra_core::Hash256;
use astra_platform::{PackageCachePolicy, PlatformErrorCode};
use astra_platform_general::VerifiedPackageCache;

#[test]
fn verified_cache_rejects_mismatched_bytes_without_committing_staging() {
    let temp = tempfile::tempdir().unwrap();
    let mut cache = VerifiedPackageCache::open(
        temp.path(),
        PackageCachePolicy {
            max_entry_bytes: 64,
            max_total_bytes: 128,
        },
    )
    .unwrap();
    let expected = Hash256::from_sha256(b"expected").to_string();
    assert_eq!(
        cache.store_verified(&expected, b"actual").unwrap_err().code,
        PlatformErrorCode::IntegrityMismatch
    );
    assert_eq!(cache.entry_count(), 0);
}

#[test]
fn verified_cache_evicts_least_recently_used_entry_before_commit() {
    let temp = tempfile::tempdir().unwrap();
    let mut cache = VerifiedPackageCache::open(
        temp.path(),
        PackageCachePolicy {
            max_entry_bytes: 8,
            max_total_bytes: 10,
        },
    )
    .unwrap();
    let first = Hash256::from_sha256(b"aaaaaa").to_string();
    let second = Hash256::from_sha256(b"bbbbbb").to_string();
    cache.store_verified(&first, b"aaaaaa").unwrap();
    cache.store_verified(&second, b"bbbbbb").unwrap();

    assert!(!cache.contains(&first).unwrap());
    assert!(cache.contains(&second).unwrap());
    assert_eq!(cache.entry_count(), 1);
}

#[test]
fn verified_cache_never_evicts_an_open_package_handle() {
    let temp = tempfile::tempdir().unwrap();
    let mut cache = VerifiedPackageCache::open(
        temp.path(),
        PackageCachePolicy {
            max_entry_bytes: 8,
            max_total_bytes: 10,
        },
    )
    .unwrap();
    let first = Hash256::from_sha256(b"aaaaaa").to_string();
    let second = Hash256::from_sha256(b"bbbbbb").to_string();
    cache.store_verified(&first, b"aaaaaa").unwrap();
    let open_handle = cache.open_source(&first).unwrap();
    assert_eq!(
        cache.store_verified(&second, b"bbbbbb").unwrap_err().code,
        PlatformErrorCode::InvalidState
    );
    drop(open_handle);
    cache.store_verified(&second, b"bbbbbb").unwrap();
    assert!(!cache.contains(&first).unwrap());
    assert!(cache.contains(&second).unwrap());
}
