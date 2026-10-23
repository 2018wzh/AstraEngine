use super::*;
use astra_emu_metadata::{IndexedFile, METADATA_INDEX_SCHEMA_VERSION};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

pub(crate) fn compatibility_json(entries: &str) -> Vec<u8> {
    format!(
        r#"{{"schema":"astra.emu.compatibility.v3","generated_at_unix_ms":1,"entries":[{entries}]}}"#
    )
    .into_bytes()
}

pub(crate) fn files(compatibility: Vec<u8>, presets: &str) -> Vec<(String, Vec<u8>)> {
    vec![
        (COMPATIBILITY_FILE.into(), compatibility),
        (
            PRESETS_FILE.into(),
            format!(r#"{{"schema":"astra.emu.presets.v1","entries":[{presets}]}}"#).into_bytes(),
        ),
        (
            CORES_FILE.into(),
            br#"{"schema":"astra.emu.cores.v1","entries":[]}"#.to_vec(),
        ),
        (
            PATCH_CATALOG_FILE.into(),
            br#"[{"id":"musica-eden-cn","title":"eden","script":"musica-eden-cn.luau","vn_ids":["v17"],"emulators":["astra.emu.musica"]}]"#.to_vec(),
        ),
        ("patches/musica-eden-cn.luau".into(), b"return {}".to_vec()),
    ]
}

pub(crate) fn index(generated_at_unix_ms: i64, files: &[(String, Vec<u8>)]) -> MetadataIndex {
    MetadataIndex {
        schema: METADATA_INDEX_SCHEMA_VERSION.into(),
        generated_at_unix_ms,
        source_commit: COMMIT.into(),
        files: files
            .iter()
            .map(|(path, bytes)| IndexedFile {
                path: path.clone(),
                sha256: sha256_hex(bytes),
                size: bytes.len() as u64,
            })
            .collect(),
    }
}

/// Write a complete metadata tree as `build_index.py` would publish it.
pub(crate) fn write_tree(root: &Path, generated_at_unix_ms: i64, files: &[(String, Vec<u8>)]) {
    for (path, bytes) in files {
        let destination = indexed_path(root, path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(destination, bytes).unwrap();
    }
    fs::write(
        root.join(METADATA_INDEX_FILE),
        serde_json::to_vec(&index(generated_at_unix_ms, files)).unwrap(),
    )
    .unwrap();
}

#[test]
fn newer_bundled_snapshot_wins_and_equal_age_prefers_synchronized_cache() {
    let data = tempfile::tempdir().unwrap();
    let bundled = tempfile::tempdir().unwrap();
    let tree = files(compatibility_json(""), "");
    assert!(select_catalog(data.path(), Some(bundled.path())).is_none());
    write_tree(bundled.path(), 20, &tree);
    write_tree(&data.path().join(METADATA_DIRECTORY), 10, &tree);
    let selected = select_catalog(data.path(), Some(bundled.path())).unwrap();
    assert_eq!(selected.source, MetadataSource::Bundled);
    assert_eq!(selected.recipes[0].id, "musica-eden-cn");
    write_tree(&data.path().join(METADATA_DIRECTORY), 20, &tree);
    assert_eq!(
        select_catalog(data.path(), Some(bundled.path()))
            .unwrap()
            .source,
        MetadataSource::Synchronized
    );
}

#[test]
fn damaged_synchronized_cache_falls_back_to_bundled_snapshot() {
    let data = tempfile::tempdir().unwrap();
    let bundled = tempfile::tempdir().unwrap();
    let tree = files(compatibility_json(""), "");
    write_tree(bundled.path(), 1, &tree);
    let cache = data.path().join(METADATA_DIRECTORY);
    write_tree(&cache, 50, &tree);
    fs::write(cache.join(PRESETS_FILE), b"{}").unwrap();
    assert_eq!(
        select_catalog(data.path(), Some(bundled.path()))
            .unwrap()
            .source,
        MetadataSource::Bundled
    );
}

#[test]
fn store_replaces_cache_and_reuses_unchanged_files() {
    let data = tempfile::tempdir().unwrap();
    let first = files(compatibility_json(""), "");
    let stored = store_synchronized(data.path(), None, &index(1, &first), first.clone()).unwrap();
    assert_eq!(stored.root, data.path().join(METADATA_DIRECTORY));
    let entry = r#"{"vn_id":"v1","release_id":"r1","emulator":"astra.emu.musica","status":"perfect","platform":"windows","updated_at_unix_ms":1}"#;
    let second = files(compatibility_json(entry), "");
    let changed = vec![second[0].clone()];
    let updated =
        store_synchronized(data.path(), Some(&stored), &index(2, &second), changed).unwrap();
    assert_eq!(updated.compatibility.entries.len(), 1);
    assert_eq!(updated.recipes.len(), 1);
    assert!(!data.path().join(STAGING_DIRECTORY).exists());
    assert!(!data.path().join(PREVIOUS_DIRECTORY).exists());
}

#[test]
fn tampered_or_unparsable_update_keeps_the_previous_cache() {
    let data = tempfile::tempdir().unwrap();
    let first = files(compatibility_json(""), "");
    let stored = store_synchronized(data.path(), None, &index(1, &first), first.clone()).unwrap();
    let entry = r#"{"vn_id":"v1","release_id":"r1","emulator":"astra.emu.musica","status":"perfect","platform":"windows","updated_at_unix_ms":1}"#;
    let second = files(compatibility_json(entry), "");
    let mut tampered = second.clone();
    tampered[0].1.push(b' ');
    let error =
        store_synchronized(data.path(), Some(&stored), &index(2, &second), tampered).unwrap_err();
    assert!(
        error.starts_with("ASTRA_EMU_METADATA_HASH_MISMATCH"),
        "{error}"
    );

    let mut unparsable = second;
    unparsable[1].1 = b"{\"schema\":\"astra.emu.presets.v0\",\"entries\":[]}".to_vec();
    let error = store_synchronized(
        data.path(),
        Some(&stored),
        &index(2, &unparsable),
        unparsable.clone(),
    )
    .unwrap_err();
    assert!(error.contains("SCHEMA_MISMATCH"), "{error}");

    let kept = select_catalog(data.path(), None).unwrap();
    assert_eq!(kept.index.generated_at_unix_ms, 1);
    assert!(kept.compatibility.entries.is_empty());
    assert!(!data.path().join(STAGING_DIRECTORY).exists());
}

/// A valid catalog written under `root/sample` for controller tests.
pub(crate) fn sample_catalog(root: &Path) -> MetadataCatalog {
    let directory = root.join("sample-metadata");
    write_tree(&directory, 1, &files(compatibility_json(""), ""));
    MetadataCatalog::load(&directory, MetadataSource::Bundled).unwrap()
}
