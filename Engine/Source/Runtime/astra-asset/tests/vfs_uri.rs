use std::fs;

use astra_asset::{
    AssetCatalog, AssetCatalogEntry, LocalMountRootSet, ResolveContext, VfsBackendKind,
    VfsCasePolicy, VfsLayerDescriptor, VfsManifest, VfsManifestEntry, VfsPrefixDescriptor,
    VfsReadWriteMode, VfsSourceRef, VfsUri, VfsWhiteoutEntry,
};
use astra_core::Hash256;

#[test]
fn vfs_uri_accepts_provider_prefix_and_normalizes_path() {
    let uri = VfsUri::parse("fvp:\\graph_bg\\BG001_000").unwrap();
    assert_eq!(uri.prefix(), "fvp");
    assert_eq!(uri.path(), "graph_bg/BG001_000");
    assert_eq!(uri.as_str(), "fvp:/graph_bg/BG001_000");
    assert_eq!(
        uri.lookup_path(VfsCasePolicy::CaseInsensitive),
        "graph_bg/bg001_000"
    );
}

#[test]
fn vfs_uri_rejects_host_paths_escape_and_inline_payload() {
    for value in [
        "package:native-assets/bg.png",
        "package:/",
        ":/native-assets/bg.png",
        "local:/../secret.txt",
        "local:/C:/secret.txt",
        "local://server/share/file.bin",
        "local:/payload:data",
        "local:/bad\u{0007}name",
    ] {
        assert!(VfsUri::parse(value).is_err(), "{value} should be rejected");
    }
}

#[test]
fn vfs_mount_descriptor_serializes_without_host_root() {
    let manifest = VfsManifest {
        schema: "astra.asset_vfs_manifest.v1".to_string(),
        prefixes: vec![VfsPrefixDescriptor {
            prefix: "local".to_string(),
            provider_id: "astra.vfs.local".to_string(),
            backend: VfsBackendKind::LocalAuthorized,
            case_policy: VfsCasePolicy::CaseSensitive,
            mode: VfsReadWriteMode::ReadOnly,
            redaction: "shipping".to_string(),
            capabilities: vec!["filesystem.authorized_read".to_string()],
        }],
        layers: vec![VfsLayerDescriptor {
            layer_id: "local.base".to_string(),
            prefix: "local".to_string(),
            priority: 0,
            source: VfsSourceRef::local_authorized("local"),
            targets: vec!["nativevn-game".to_string()],
            profiles: vec!["classic".to_string()],
        }],
        entries: vec![VfsManifestEntry {
            uri: VfsUri::parse("local:/probe/manifest.json").unwrap(),
            layer_id: "local.base".to_string(),
            source: VfsSourceRef::local_authorized("local"),
            offset: 0,
            size: 2,
            hash: Hash256::from_sha256(b"ok"),
            codec: None,
            media_kind: "json".to_string(),
            diagnostics: vec![],
        }],
        whiteouts: vec![],
    };

    let encoded = serde_json::to_string(&manifest).unwrap();
    assert!(encoded.contains("local:/probe/manifest.json"));
    assert!(!encoded.contains("C:"));
    assert!(!encoded.contains("Users"));
    assert!(manifest.validate().is_empty());
}

#[test]
fn vfs_overlayfs_resolves_highest_priority_and_whiteout() {
    let hash_old = Hash256::from_sha256(b"old");
    let hash_new = Hash256::from_sha256(b"new");
    let uri = VfsUri::parse("package:/native-assets/bg.png").unwrap();
    let hidden = VfsUri::parse("package:/native-assets/old.png").unwrap();
    let manifest = VfsManifest {
        schema: "astra.asset_vfs_manifest.v1".to_string(),
        prefixes: vec![VfsPrefixDescriptor {
            prefix: "package".to_string(),
            provider_id: "astra.vfs.package".to_string(),
            backend: VfsBackendKind::Package,
            case_policy: VfsCasePolicy::CaseSensitive,
            mode: VfsReadWriteMode::ReadOnly,
            redaction: "shipping".to_string(),
            capabilities: vec!["package.read".to_string()],
        }],
        layers: vec![
            VfsLayerDescriptor {
                layer_id: "base".to_string(),
                prefix: "package".to_string(),
                priority: 0,
                source: VfsSourceRef::package_section("asset.bg.base"),
                targets: vec![],
                profiles: vec![],
            },
            VfsLayerDescriptor {
                layer_id: "patch".to_string(),
                prefix: "package".to_string(),
                priority: 10,
                source: VfsSourceRef::package_section("asset.bg.patch"),
                targets: vec![],
                profiles: vec![],
            },
        ],
        entries: vec![
            VfsManifestEntry {
                uri: uri.clone(),
                layer_id: "base".to_string(),
                source: VfsSourceRef::package_section("asset.bg.base"),
                offset: 0,
                size: 3,
                hash: hash_old,
                codec: None,
                media_kind: "image".to_string(),
                diagnostics: vec![],
            },
            VfsManifestEntry {
                uri: uri.clone(),
                layer_id: "patch".to_string(),
                source: VfsSourceRef::package_section("asset.bg.patch"),
                offset: 0,
                size: 3,
                hash: hash_new,
                codec: None,
                media_kind: "image".to_string(),
                diagnostics: vec![],
            },
        ],
        whiteouts: vec![VfsWhiteoutEntry {
            uri: hidden.clone(),
            layer_id: "patch".to_string(),
            base_hash: Hash256::from_sha256(b"hidden"),
            allowlist_id: "patch.native-assets".to_string(),
            reason: "remove obsolete patch target".to_string(),
            targets: vec!["nativevn-game".to_string()],
            profiles: vec!["classic".to_string()],
        }],
    };

    let context = ResolveContext {
        target: "nativevn-game".to_string(),
        profile: "classic".to_string(),
        capability: "package.read".to_string(),
        provider_binding: "astra.vfs.package".to_string(),
    };
    let resolved = manifest.resolve(&uri, &context).unwrap().unwrap();
    assert_eq!(resolved.layer_id, "patch");
    assert_eq!(resolved.hash, hash_new);
    assert!(manifest.resolve(&hidden, &context).unwrap().is_none());
}

#[test]
fn vfs_local_reader_uses_host_capability_without_serializing_root() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("probe")).unwrap();
    fs::write(temp.path().join("probe").join("manifest.json"), b"ok").unwrap();

    let mut roots = LocalMountRootSet::default();
    roots.authorize("local", temp.path()).unwrap();
    let uri = VfsUri::parse("local:/probe/manifest.json").unwrap();
    let bytes = roots
        .read_bounded(&uri, 4, Some(Hash256::from_sha256(b"ok")))
        .unwrap();
    assert_eq!(bytes, b"ok");
    assert!(roots
        .read_bounded(&uri, 1, Some(Hash256::from_sha256(b"ok")))
        .is_err());

    let encoded = serde_json::to_string(&AssetCatalog {
        schema: "astra.asset_catalog.v1".to_string(),
        assets: vec![AssetCatalogEntry {
            asset_id: "asset:/probe/manifest".to_string(),
            uri,
            media_kind: "json".to_string(),
            tags: vec!["probe".to_string()],
            bundle_id: Some("classic".to_string()),
            chunk_id: Some("base".to_string()),
            profiles: vec!["classic".to_string()],
        }],
    })
    .unwrap();
    assert!(encoded.contains("local:/probe/manifest.json"));
    assert!(!encoded.contains(temp.path().to_string_lossy().as_ref()));
}

#[test]
fn vfs_manifest_blocks_duplicate_graph_nodes_and_context_bypass() {
    let mut duplicate_prefix = package_manifest_fixture();
    duplicate_prefix
        .prefixes
        .push(duplicate_prefix.prefixes[0].clone());
    assert!(duplicate_prefix
        .validate()
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VFS_PREFIX_DUPLICATE"));

    let mut duplicate_layer = package_manifest_fixture();
    duplicate_layer
        .layers
        .push(duplicate_layer.layers[0].clone());
    assert!(duplicate_layer
        .validate()
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VFS_LAYER_DUPLICATE"));

    let mut duplicate_entry = package_manifest_fixture();
    duplicate_entry
        .entries
        .push(duplicate_entry.entries[0].clone());
    assert!(duplicate_entry
        .validate()
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VFS_ENTRY_DUPLICATE"));

    let manifest = package_manifest_fixture();
    let uri = VfsUri::parse("package:/asset.bin").unwrap();
    for context in [
        ResolveContext {
            target: "other".into(),
            profile: "release".into(),
            capability: "package.read".into(),
            provider_binding: "astra.vfs.package".into(),
        },
        ResolveContext {
            target: "game".into(),
            profile: "release".into(),
            capability: "filesystem.read".into(),
            provider_binding: "astra.vfs.package".into(),
        },
        ResolveContext {
            target: "game".into(),
            profile: "release".into(),
            capability: "package.read".into(),
            provider_binding: "astra.vfs.other".into(),
        },
    ] {
        assert!(manifest.resolve(&uri, &context).is_err());
    }
}

#[test]
fn vfs_resolve_rejects_equal_priority_authority_conflict() {
    let mut manifest = package_manifest_fixture();
    manifest.layers.push(VfsLayerDescriptor {
        layer_id: "second".into(),
        prefix: "package".into(),
        priority: 0,
        source: VfsSourceRef::package_section("asset.second"),
        targets: vec!["game".into()],
        profiles: vec!["release".into()],
    });
    let mut second = manifest.entries[0].clone();
    second.layer_id = "second".into();
    second.source = VfsSourceRef::package_section("asset.second");
    manifest.entries.push(second);
    let error = manifest
        .resolve(
            &VfsUri::parse("package:/asset.bin").unwrap(),
            &ResolveContext {
                target: "game".into(),
                profile: "release".into(),
                capability: "package.read".into(),
                provider_binding: "astra.vfs.package".into(),
            },
        )
        .unwrap_err();
    assert!(error
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VFS_RESOLVE_AMBIGUOUS"));
}

fn package_manifest_fixture() -> VfsManifest {
    VfsManifest {
        schema: "astra.asset_vfs_manifest.v1".into(),
        prefixes: vec![VfsPrefixDescriptor {
            prefix: "package".into(),
            provider_id: "astra.vfs.package".into(),
            backend: VfsBackendKind::Package,
            case_policy: VfsCasePolicy::CaseSensitive,
            mode: VfsReadWriteMode::ReadOnly,
            redaction: "shipping".into(),
            capabilities: vec!["package.read".into()],
        }],
        layers: vec![VfsLayerDescriptor {
            layer_id: "base".into(),
            prefix: "package".into(),
            priority: 0,
            source: VfsSourceRef::package_section("asset.base"),
            targets: vec!["game".into()],
            profiles: vec!["release".into()],
        }],
        entries: vec![VfsManifestEntry {
            uri: VfsUri::parse("package:/asset.bin").unwrap(),
            layer_id: "base".into(),
            source: VfsSourceRef::package_section("asset.base"),
            offset: 0,
            size: 5,
            hash: Hash256::from_sha256(b"asset"),
            codec: None,
            media_kind: "binary".into(),
            diagnostics: vec![],
        }],
        whiteouts: vec![],
    }
}
