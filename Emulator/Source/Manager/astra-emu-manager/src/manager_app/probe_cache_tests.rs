use super::*;
use crate::test_native::{self, Fixture};
use std::sync::atomic::AtomicBool;

fn registry() -> FamilyProviderRegistry {
    let mut registry = FamilyProviderRegistry::new();
    let mut fixture = Fixture::new("cache-test", "fixture", "fixture");
    fixture.marker = Some("boot");
    registry
        .register_provider(test_native::install(0, fixture))
        .unwrap();
    registry
}

fn game(path: &Path) -> GameRecord {
    GameRecord {
        game_id: "registered".into(),
        title: "Saved title".into(),
        user_title: Some("Alias".into()),
        location: path.to_str().unwrap().into(),
        family_id: Some("fixture".into()),
        content_fingerprint: None,
        added_at_unix_ms: 1,
    }
}

fn refresh(
    registry: &FamilyProviderRegistry,
    cache: &mut ProbeCache,
    path: &Path,
) -> super::super::probe_worker::ScanReport {
    super::super::probe_worker::scan(
        registry,
        cache,
        super::super::probe_worker::ScanRequest {
            kind: super::super::probe_worker::ScanKind::Full,
            paths: Vec::new(),
            games: vec![game(path)],
        },
        &AtomicBool::new(false),
    )
    .unwrap()
}

#[test]
fn manual_full_scan_reuses_registered_hints_until_boot_or_core_inventory_changes() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let path = root.path().join("game");
    fs::create_dir(&cores).unwrap();
    fs::create_dir(&path).unwrap();
    fs::write(path.join("boot"), b"boot").unwrap();
    let calls = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let mut fixture = Fixture::new("cache-test", "fixture", "fixture");
    fixture.marker = Some("boot");
    fixture.calls = calls.clone();
    let mut registry = FamilyProviderRegistry::new();
    registry
        .register_provider(test_native::install(0, fixture))
        .unwrap();
    let inventory = Inventory::read(std::slice::from_ref(&cores), &registry).unwrap();
    let mut cache = ProbeCache::load(root.path(), Some(inventory.clone()));
    let first = refresh(&registry, &mut cache, &path);
    assert_eq!(first.cache_hits, 0);
    assert_eq!(first.games[0].candidates.len(), 1);
    assert!(first.cache_error.is_none());
    assert_eq!(
        calls
            .lock()
            .iter()
            .filter(|call| call.name == "probe")
            .count(),
        1
    );
    let mut loaded = ProbeCache::load(root.path(), Some(inventory));
    let hit = refresh(&registry, &mut loaded, &path);
    assert_eq!(hit.cache_hits, 1);
    assert_eq!(hit.games[0].candidates, first.games[0].candidates);
    assert_eq!(
        calls
            .lock()
            .iter()
            .filter(|call| call.name == "probe")
            .count(),
        1
    );
    fs::write(path.join("boot"), b"changed boot contents").unwrap();
    assert_eq!(refresh(&registry, &mut loaded, &path).cache_hits, 0);
    assert_eq!(
        calls
            .lock()
            .iter()
            .filter(|call| call.name == "probe")
            .count(),
        2
    );
    fs::write(cores.join("new-core.dll"), b"new inventory member").unwrap();
    let changed = Inventory::read(&[cores], &registry).unwrap();
    let mut loaded = ProbeCache::load(root.path(), Some(changed));
    assert_eq!(refresh(&registry, &mut loaded, &path).cache_hits, 0);
    assert_eq!(
        calls
            .lock()
            .iter()
            .filter(|call| call.name == "probe")
            .count(),
        3
    );
    fs::remove_file(path.join("boot")).unwrap();
    let removed = refresh(&registry, &mut loaded, &path);
    assert!(removed.games[0].candidates.is_empty());
    assert!(loaded.get(&path, &registry).is_none());
}

#[test]
fn malformed_cache_and_unknown_deep_layouts_are_misses_and_missing_roots_are_pruned() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("game");
    fs::create_dir(&path).unwrap();
    fs::write(path.join("boot"), b"boot").unwrap();
    let registry = registry();
    let inventory = Inventory::read(&[], &registry).unwrap();
    fs::write(root.path().join("probe-cache.json"), b"{broken").unwrap();
    let mut cache = ProbeCache::load(root.path(), Some(inventory));
    assert_eq!(refresh(&registry, &mut cache, &path).cache_hits, 0);
    fs::create_dir_all(path.join("unknown").join("nested")).unwrap();
    assert_eq!(refresh(&registry, &mut cache, &path).cache_hits, 0);
    assert!(cache.get(&path, &registry).is_none());
    fs::remove_dir_all(path.join("unknown")).unwrap();
    refresh(&registry, &mut cache, &path);
    assert!(cache.get(&path, &registry).is_some());
    fs::remove_dir_all(&path).unwrap();
    let missing = refresh(&registry, &mut cache, &path);
    assert_eq!(missing.missing, 1);
    assert!(cache.entries.is_empty());
}

#[test]
fn manual_full_scan_discovers_unregistered_children_with_fresh_probes() {
    let root = tempfile::tempdir().unwrap();
    let child = root.path().join("unregistered-game");
    fs::create_dir(&child).unwrap();
    fs::write(child.join("boot"), b"boot").unwrap();
    let calls = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let mut fixture = Fixture::new("cache-test", "fixture", "fixture");
    fixture.marker = Some("boot");
    fixture.calls = calls.clone();
    let mut registry = FamilyProviderRegistry::new();
    registry
        .register_provider(test_native::install(0, fixture))
        .unwrap();
    let mut cache = ProbeCache::load(root.path(), Inventory::read(&[], &registry));
    // A hint from an earlier import cannot identify an unregistered discovery.
    cache.probe(&child, &registry, false).unwrap();
    let before = calls
        .lock()
        .iter()
        .filter(|call| call.name == "probe")
        .count();
    let report = super::super::probe_worker::scan(
        &registry,
        &mut cache,
        super::super::probe_worker::ScanRequest {
            kind: super::super::probe_worker::ScanKind::Full,
            paths: vec![root.path().into()],
            games: Vec::new(),
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(report.cache_hits, 0);
    assert_eq!(report.games.len(), 1);
    assert_eq!(report.games[0].path, fs::canonicalize(&child).unwrap());
    assert!(report.games[0].game_ids.is_empty());
    assert_eq!(report.games[0].candidates.len(), 1);
    assert!(
        calls
            .lock()
            .iter()
            .filter(|call| call.name == "probe")
            .count()
            > before
    );
}

#[test]
fn explicit_import_always_probes_registered_paths_freshly() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("game");
    fs::create_dir(&path).unwrap();
    fs::write(path.join("boot"), b"boot").unwrap();
    let calls = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let mut fixture = Fixture::new("cache-test", "fixture", "fixture");
    fixture.marker = Some("boot");
    fixture.calls = calls.clone();
    let mut registry = FamilyProviderRegistry::new();
    registry
        .register_provider(test_native::install(0, fixture))
        .unwrap();
    let mut cache = ProbeCache::load(root.path(), Inventory::read(&[], &registry));
    refresh(&registry, &mut cache, &path);
    let before = calls
        .lock()
        .iter()
        .filter(|call| call.name == "probe")
        .count();
    let report = super::super::probe_worker::scan(
        &registry,
        &mut cache,
        super::super::probe_worker::ScanRequest {
            kind: super::super::probe_worker::ScanKind::Import,
            paths: vec![path.clone()],
            games: vec![game(&path)],
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(report.cache_hits, 0);
    assert_eq!(report.games[0].game_ids, ["registered"]);
    assert_eq!(report.games[0].candidates.len(), 1);
    assert_eq!(
        calls
            .lock()
            .iter()
            .filter(|call| call.name == "probe")
            .count(),
        before + 1
    );
}

#[test]
fn cached_selection_is_freshly_rejected_before_open_without_probing_alternatives() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("game");
    fs::create_dir(&path).unwrap();
    fs::write(path.join("boot"), b"boot").unwrap();
    let calls = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let mut first = Fixture::new("chosen", "fixture", "fixture");
    first.marker = Some("boot");
    first.calls = calls.clone();
    let other_calls = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let mut other = Fixture::new("alternative", "fixture", "fixture");
    other.calls = other_calls.clone();
    let mut registry = FamilyProviderRegistry::new();
    registry
        .register_provider(test_native::install(0, first))
        .unwrap();
    registry
        .register_provider(test_native::install(1, other))
        .unwrap();
    let request = ProbeRequest {
        game_path: path.to_str().unwrap().into(),
    };
    let inventory = Inventory::read(&[], &registry).unwrap();
    let mut cache = ProbeCache::load(root.path(), Some(inventory.clone()));
    cache.probe(&path, &registry, false).unwrap();
    cache.save().unwrap();
    let cache = ProbeCache::load(root.path(), Some(inventory));
    let hint = cache
        .get(&path, &registry)
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.report.plugin_id == "chosen")
        .unwrap();
    let chosen_before = calls
        .lock()
        .iter()
        .filter(|call| call.name == "probe")
        .count();
    let other_before = other_calls
        .lock()
        .iter()
        .filter(|call| call.name == "probe")
        .count();
    fs::remove_file(path.join("boot")).unwrap();
    let epoch = astra_emu_manager_core::model::allocate_session_epoch().unwrap();
    let cancellation = astra_emu_manager_core::NativeCancellation::new(epoch);
    let result = registry.open_selected(
        &hint,
        NativeOpenRequest {
            game_path: request.game_path,
            session_epoch: epoch,
            configuration: Vec::new(),
            initial_window: WindowState {
                width: 1,
                height: 1,
                scale_factor: 1.0,
                focused: true,
                visible: true,
                suspended: false,
            },
            host: astra_emu_manager_core::no_host_services(&cancellation),
            cancellation,
            render_extent: None,
        },
    );
    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .contains("ASTRA_EMU_FAMILY_FRESH_PROBE_NO_MATCH")
    );
    assert_eq!(
        calls
            .lock()
            .iter()
            .filter(|call| call.name == "probe")
            .count(),
        chosen_before + 1
    );
    assert_eq!(
        other_calls
            .lock()
            .iter()
            .filter(|call| call.name == "probe")
            .count(),
        other_before
    );
    assert!(!calls.lock().iter().any(|call| call.name == "open"));
}

#[test]
fn rejected_duplicate_provider_cannot_be_revived_by_a_persistent_hint() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("game");
    fs::create_dir(&path).unwrap();
    fs::write(path.join("boot"), b"boot").unwrap();
    let mut registry = registry();
    let mut cache = ProbeCache::load(root.path(), Inventory::read(&[], &registry));
    refresh(&registry, &mut cache, &path);
    assert!(cache.get(&path, &registry).is_some());
    let duplicate = Fixture::new("cache-test", "fixture", "fixture");
    assert!(
        registry
            .register_provider(test_native::install(1, duplicate))
            .is_err()
    );
    assert!(registry.descriptor("cache-test").is_none());
    assert!(cache.get(&path, &registry).is_none());
    assert!(
        registry
            .probe_selected(
                &ProbeRequest {
                    game_path: path.to_str().unwrap().into()
                },
                "cache-test"
            )
            .is_err()
    );
}

#[test]
fn owned_musica_saves_do_not_invalidate_game_identification() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("game");
    let saves = path.join(".astra-musica").join("saves");
    fs::create_dir_all(&saves).unwrap();
    fs::write(path.join("boot"), b"boot").unwrap();
    let registry = registry();
    let mut cache = ProbeCache::load(root.path(), Inventory::read(&[], &registry));
    assert_eq!(refresh(&registry, &mut cache, &path).cache_hits, 0);
    fs::write(saves.join("slot-01"), b"new save data").unwrap();
    assert_eq!(refresh(&registry, &mut cache, &path).cache_hits, 1);
    fs::write(path.join("boot"), b"changed game assets").unwrap();
    assert_eq!(refresh(&registry, &mut cache, &path).cache_hits, 0);
}

#[test]
fn already_registered_nested_games_are_independently_refreshed() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("parent-game");
    let child = path.join("child-game");
    fs::create_dir_all(&child).unwrap();
    fs::write(path.join("boot"), b"parent").unwrap();
    fs::write(child.join("boot"), b"child").unwrap();
    let registry = registry();
    let mut cache = ProbeCache::load(root.path(), Inventory::read(&[], &registry));
    let mut nested = game(&child);
    nested.game_id = "nested".into();
    let report = super::super::probe_worker::scan(
        &registry,
        &mut cache,
        super::super::probe_worker::ScanRequest {
            kind: super::super::probe_worker::ScanKind::Full,
            paths: Vec::new(),
            games: vec![game(&path), nested],
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(report.games.len(), 2);
    for (index, expected) in ["registered", "nested"].iter().enumerate() {
        assert_eq!(report.games[index].game_ids, [*expected]);
        assert_eq!(report.games[index].candidates.len(), 1);
    }
}
