use astra_core::Hash256;
use astra_package::{PackageBuildRequest, PackageBuilder, PackageReader, SectionPayload};

#[test]
fn package_vfs_mount_writes_vfs_manifest_and_catalog_without_asset_registry() {
    let cooked = SectionPayload::raw(
        "asset.background.opening",
        "astra.cooked_asset.v1",
        b"opening".to_vec(),
    );
    let expected_hash = Hash256::from_sha256(&cooked.payload).to_string();
    let blob = PackageBuilder::build(PackageBuildRequest::minimal(
        "com.example.nativevn",
        "classic",
        vec![cooked],
    ))
    .unwrap();

    let package = PackageReader::open(blob.as_bytes()).unwrap();
    assert!(package.has_section("asset.vfs_manifest"));
    assert!(package.has_section("asset.catalog"));
    assert!(!package.has_section("asset.registry"));

    let manifest: serde_json::Value = serde_json::from_slice(
        &package
            .container()
            .read_section("asset.vfs_manifest")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["schema"], "astra.asset_vfs_manifest.v1");
    assert_eq!(manifest["prefixes"][0]["prefix"], "package");
    assert_eq!(manifest["prefixes"][0]["provider_id"], "astra.vfs.package");
    assert_eq!(
        manifest["entries"][0]["vfs_uri"],
        "package:/asset/background/opening"
    );
    assert_eq!(manifest["entries"][0]["hash"], expected_hash);
    assert_eq!(
        manifest["entries"][0]["source"]["section_id"],
        "asset.background.opening"
    );

    let catalog: serde_json::Value =
        serde_json::from_slice(&package.container().read_section("asset.catalog").unwrap())
            .unwrap();
    assert_eq!(catalog["schema"], "astra.asset_catalog.v1");
    assert_eq!(
        catalog["assets"][0]["vfs_uri"],
        "package:/asset/background/opening"
    );
}
