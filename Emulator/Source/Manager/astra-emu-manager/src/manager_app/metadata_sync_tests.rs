use super::*;
use crate::metadata_cache::tests::{compatibility_json, files, index, sample_catalog};
use astra_emu_manager_core::{ExternalIdentityRecord, FamilyProbeReport, INDEPENDENT_FAMILY_ABI_FINGERPRINT};

const MUSICA: &str = "astra.emu.musica";
const SENA: &str = "sena-native-koikake";

fn controller(root: &Path) -> AstraEmuManagerController {
    AstraEmuManagerController::open_library(root.into(), FrameMailbox::new(), Vec::new()).unwrap()
}

fn entry(release_id: &str, emulator: &str, status: &str) -> String {
    format!(
        r#"{{"vn_id":"v17","release_id":"{release_id}","emulator":"{emulator}","status":"{status}","notes":"note","platform":"windows","updated_at_unix_ms":1}}"#
    )
}

fn catalog_with(root: &Path, entries: &[String], presets: &str) -> MetadataCatalog {
    let directory = root.join("catalog");
    let tree = files(compatibility_json(&entries.join(",")), presets);
    crate::metadata_cache::tests::write_tree(&directory, 1, &tree);
    MetadataCatalog::load(&directory, MetadataSource::Bundled).unwrap()
}

fn candidate(plugin_id: &str) -> FamilyProbeCandidate {
    FamilyProbeCandidate {
        report: FamilyProbeReport {
            plugin_id: plugin_id.into(),
            family_id: "family".into(),
            game_id: "game".into(),
            format: "format".into(),
            confidence_permyriad: 10_000,
        },
        descriptor: FamilyPluginDescriptor {
            family_id: "family".into(),
            plugin_id: plugin_id.into(),
            abi_fingerprint: INDEPENDENT_FAMILY_ABI_FINGERPRINT.into(),
            version: "0.2.0".into(),
            capabilities: vec![astra_emu_manager_core::FamilyCapability::CpuFrame],
            supported_formats: vec!["format".into()],
            display_name: plugin_id.into(),
            family_name: "family".into(),
            render: None,
        },
    }
}

fn linked_game(app: &mut AstraEmuManagerController, game_id: &str, emulator: Option<&str>) {
    app.library
        .add_game(&GameRecord {
            game_id: game_id.into(),
            title: "Game".into(),
            user_title: None,
            location: format!("fixtures/{game_id}"),
            family_id: Some("family".into()),
            content_fingerprint: None,
            added_at_unix_ms: 1,
        })
        .unwrap();
    app.library
        .set_external_identity(&ExternalIdentityRecord {
            game_id: game_id.into(),
            provider: "vndb".into(),
            remote_id: "v17".into(),
            provenance: "test".into(),
            linked_at_unix_ms: 1,
        })
        .unwrap();
    if let Some(emulator) = emulator {
        app.candidates.insert(game_id.into(), candidate(emulator));
    }
}

fn identities(app: &AstraEmuManagerController, game_id: &str) -> Vec<ExternalIdentityRecord> {
    app.library.external_identities(game_id).unwrap()
}

#[test]
fn pinned_release_and_current_emulator_select_the_compatibility_record() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    app.metadata_catalog = Some(catalog_with(
        root.path(),
        &[entry("r1", MUSICA, "perfect"), entry("r1", SENA, "flawed")],
        "",
    ));
    linked_game(&mut app, "game", Some(SENA));
    app.pinned_releases.insert("game".into(), "r1".into());
    let linked = identities(&app, "game");
    assert_eq!(app.compatibility_status("game", &linked), "flawed");
    let view = app.compat_view("game", &linked, 2);
    assert_eq!(view.status, "flawed");
    assert_eq!(view.release_id, "r1");
    assert!(view.hint.is_empty());
}

#[test]
fn single_release_is_used_without_a_pin() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    app.metadata_catalog = Some(catalog_with(
        root.path(),
        &[entry("r1", MUSICA, "perfect"), entry("r1", SENA, "flawed")],
        "",
    ));
    linked_game(&mut app, "game", Some(MUSICA));
    let linked = identities(&app, "game");
    assert_eq!(app.compatibility_status("game", &linked), "perfect");
}

#[test]
fn several_releases_without_a_pin_are_undecided() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    app.metadata_catalog = Some(catalog_with(
        root.path(),
        &[entry("r1", MUSICA, "perfect"), entry("r2", MUSICA, "flawed")],
        "",
    ));
    linked_game(&mut app, "game", Some(MUSICA));
    let linked = identities(&app, "game");
    assert_eq!(app.compatibility_status("game", &linked), "");
    let view = app.compat_view("game", &linked, 2);
    assert_eq!(view.hint, RELEASE_HINT);
    assert!(view.status.is_empty());

    app.candidates.remove("game");
    app.pinned_releases.insert("game".into(), "r1".into());
    assert_eq!(
        app.compatibility_match("game", &linked),
        CompatibilityMatch::NoEmulator {
            release_id: "r1".into()
        }
    );
}

#[test]
fn report_url_carries_release_emulator_and_versions() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    app.metadata_catalog = Some(catalog_with(root.path(), &[entry("r1", SENA, "flawed")], ""));
    linked_game(&mut app, "game", Some(SENA));
    let url = app.report_url("game", &identities(&app, "game"));
    assert_eq!(
        url,
        format!(
            "https://github.com/2018wzh/AstraEMU-metadata/issues/new?template=compatibility.yml&vid=v17&rid=r1&emulator=sena-native-koikake&platform=windows&emulator_version=0.2.0&manager_version={}",
            env!("CARGO_PKG_VERSION")
        )
    );
    app.library.add_game(&GameRecord {
        game_id: "unlinked".into(),
        title: "Unlinked".into(),
        user_title: None,
        location: "fixtures/unlinked".into(),
        family_id: None,
        content_fingerprint: None,
        added_at_unix_ms: 1,
    })
    .unwrap();
    let view = app.compat_view("unlinked", &[], 2);
    assert!(view.report_url.is_empty());
    assert_eq!(view.hint, VNDB_LINK_HINT);
}

#[test]
fn recommended_patch_is_marked_in_the_recipe_list() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    let preset = format!(
        r#"{{"vn_id":"v17","release_id":"r1","emulator":"{MUSICA}","summary":"推荐","configuration":{{}},"patch":"musica-eden-cn"}}"#
    );
    app.metadata_catalog = Some(catalog_with(root.path(), &[], &preset));
    linked_game(&mut app, "game", Some(MUSICA));
    assert_eq!(app.recommended_preset("game").unwrap().unwrap().summary, "推荐");
    let recipes = app.recipe_views(Some("game")).unwrap();
    assert_eq!(recipes.len(), 1);
    assert!(recipes[0].recommended);
    assert!(!app.recipe_views(None).unwrap()[0].recommended);
    assert_eq!(
        app.patch_catalog_directory().unwrap(),
        root.path().join("catalog").join("patches")
    );
}

#[test]
fn metadata_sync_requires_network_permission() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    assert_eq!(
        app.sync_metadata().unwrap_err(),
        "ASTRA_EMU_NETWORK_PERMISSION_REQUIRED"
    );
    assert!(app.pending_metadata.is_empty());
    assert_eq!(app.network_view(0).sync_status, "未同步");
}

#[test]
fn rejected_update_keeps_the_previous_catalog_and_sync_record() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    let first = files(compatibility_json(""), "");
    app.apply_sync_outcome(SyncOutcome::Updated {
        index: index(1, &first),
        etag: Some("\"one\"".into()),
        files: first,
    })
    .unwrap();
    assert_eq!(app.snackbar.view().text, "元数据已更新");
    let serial = app.snackbar.view().serial;
    let record = app.metadata_sync.record.clone();
    assert_eq!(record.etag.as_deref(), Some("\"one\""));

    let second = files(compatibility_json(&entry("r1", MUSICA, "perfect")), "");
    let mut tampered = second.clone();
    tampered[0].1.push(b' ');
    let error = app
        .apply_sync_outcome(SyncOutcome::Updated {
            index: index(2, &second),
            etag: Some("\"two\"".into()),
            files: tampered,
        })
        .unwrap_err();
    assert!(error.starts_with("ASTRA_EMU_METADATA_HASH_MISMATCH"), "{error}");
    let kept = app.metadata_catalog.as_ref().unwrap();
    assert_eq!(kept.index.generated_at_unix_ms, 1);
    assert!(kept.compatibility.entries.is_empty());
    assert_eq!(app.metadata_sync.record, record);
    assert_eq!(app.snackbar.view().serial, serial);

    app.apply_sync_outcome(SyncOutcome::Unchanged).unwrap();
    assert_eq!(app.snackbar.view().serial, serial);
    drop(app);
    let reopened = controller(root.path());
    assert_eq!(
        reopened.metadata_sync.record.etag.as_deref(),
        Some("\"one\"")
    );
    assert_eq!(
        reopened.metadata_catalog.as_ref().unwrap().source,
        MetadataSource::Synchronized
    );
}

#[test]
fn sample_catalog_loads_patch_recipes() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(sample_catalog(root.path()).recipes.len(), 1);
}
