use super::*;
use metadata_runtime::MetadataCompletion;
use std::sync::mpsc;

struct Harness {
    app: AstraEmuManagerController,
    commands: mpsc::Receiver<MetadataCommand>,
    completions: mpsc::Sender<MetadataCompletion>,
    _root: tempfile::TempDir,
}

fn harness(network: bool) -> Harness {
    let root = tempfile::tempdir().unwrap();
    let mut app = AstraEmuManagerController::open_library(
        root.path().into(),
        FrameMailbox::new(),
        Vec::new(),
    )
    .unwrap();
    let (runtime, commands, completions) = MetadataRuntime::scripted();
    app.metadata = runtime;
    app.network_allowed = network;
    Harness {
        app,
        commands,
        completions,
        _root: root,
    }
}

impl Harness {
    fn add_game(&mut self, id: &str, title: &str) {
        self.app
            .library
            .add_game(&GameRecord {
                game_id: id.into(),
                title: title.into(),
                user_title: None,
                location: format!("fixtures/{id}"),
                family_id: None,
                content_fingerprint: None,
                added_at_unix_ms: 1,
            })
            .unwrap();
    }

    fn submitted(&self) -> Option<MetadataCommand> {
        self.commands.try_recv().ok()
    }

    fn complete(&mut self, command: MetadataCommand, result: Result<MetadataPayload, String>) {
        self.completions
            .send(MetadataCompletion {
                request_id: command.request_id,
                case_identity: command.case_identity,
                provider: command.provider,
                result,
            })
            .unwrap();
        assert!(self.app.poll_metadata().unwrap());
    }

    fn vndb_remote_id(&self, game_id: &str) -> Option<String> {
        self.app
            .library
            .external_identities(game_id)
            .unwrap()
            .into_iter()
            .find(|identity| identity.provider == "vndb")
            .map(|identity| identity.remote_id)
    }

    fn status(&self, game_id: &str) -> &str {
        self.app
            .metadata_status
            .get(game_id)
            .map_or("", String::as_str)
    }

    fn candidates(&self, game_id: &str) -> usize {
        self.app
            .match_candidates
            .values()
            .filter(|(id, _)| id == game_id)
            .count()
    }
}

fn record(remote_id: &str, title: &str, alternate: &[&str]) -> MetadataRecord {
    MetadataRecord {
        provider: MetadataProviderId::Vndb,
        remote_id: remote_id.into(),
        title: title.into(),
        description: None,
        alternate_titles: alternate.iter().map(|title| (*title).into()).collect(),
        developers: Vec::new(),
        tags: Vec::new(),
        release_date: None,
        platforms: Vec::new(),
        engine: None,
        cover: None,
        sensitive: false,
    }
}

fn search_title(command: &MetadataCommand) -> &str {
    match &command.kind {
        MetadataCommandKind::Search(query) => &query.title,
        other => panic!("expected search, got {other:?}"),
    }
}

#[test]
fn unique_normalized_match_is_fetched_and_linked() {
    let mut h = harness(true);
    h.add_game("first", "サクラノ詩 －櫻の森の上を舞う－");
    h.app.enqueue_auto_link().unwrap();
    let search = h.submitted().unwrap();
    assert_eq!(search.provider, MetadataProviderId::Vndb);
    assert_eq!(search_title(&search), "サクラノ詩 －櫻の森の上を舞う－");
    assert!(h.submitted().is_none());
    h.complete(
        search,
        Ok(MetadataPayload::Search(vec![
            record("v1", "Sakura no Uta", &["サクラノ詩　-櫻の森の上を舞う-"]),
            record("v2", "Sakura no Toki", &["サクラノ刻"]),
        ])),
    );
    let fetch = h.submitted().unwrap();
    assert!(matches!(&fetch.kind, MetadataCommandKind::Fetch(id) if id == "v1"));
    assert!(h.vndb_remote_id("first").is_none());
    assert!(!h.app.auto_link_attempted.contains("first"));
    h.complete(
        fetch,
        Ok(MetadataPayload::Fetch {
            record: Box::new(record("v1", "Sakura no Uta", &[])),
            cover: None,
        }),
    );
    assert_eq!(h.vndb_remote_id("first").as_deref(), Some("v1"));
    let identity = h.app.library.external_identities("first").unwrap();
    assert_eq!(identity[0].provenance, "metadata-provider");
    assert_eq!(h.status("first"), "已自动关联：Sakura no Uta");
    assert_eq!(h.candidates("first"), 0);
    assert!(h.app.auto_link_attempted.contains("first"));
    assert!(h.app.auto_link.is_none());
}

#[test]
fn ambiguous_or_missing_matches_keep_candidates_and_continue_the_batch() {
    let mut h = harness(true);
    h.add_game("a", "Air");
    h.add_game("b", "Kanon");
    h.app.enqueue_auto_link().unwrap();
    let first = h.submitted().unwrap();
    assert_eq!(search_title(&first), "Air");
    assert!(h.submitted().is_none());
    h.complete(
        first,
        Ok(MetadataPayload::Search(vec![
            record("v1", "AIR", &[]),
            record("v2", "Air (2005)", &["ＡＩＲ"]),
        ])),
    );
    assert!(h.vndb_remote_id("a").is_none());
    assert_eq!(h.candidates("a"), 2);
    assert_eq!(h.status("a"), "未能自动关联，找到 2 个候选，请手动选择");
    let second = h.submitted().unwrap();
    assert_eq!(search_title(&second), "Kanon");
    h.complete(second, Ok(MetadataPayload::Search(Vec::new())));
    assert_eq!(h.status("b"), "未在 VNDB 找到匹配作品");
    assert!(h.submitted().is_none());
    assert!(h.app.auto_link_attempted.contains("a"));
    assert!(h.app.auto_link_attempted.contains("b"));
}

#[test]
fn nothing_is_queued_without_network_permission() {
    let mut h = harness(false);
    h.add_game("first", "Air");
    h.app.enqueue_auto_link().unwrap();
    assert!(h.submitted().is_none());
    assert!(h.app.auto_link_queue.is_empty());
    assert!(h.app.pending_metadata.is_empty());
    h.app.set_network_allowed(true).unwrap();
    let search = h.submitted().unwrap();
    assert_eq!(search_title(&search), "Air");
}

#[test]
fn attempted_games_are_not_searched_again_after_restart() {
    let mut h = harness(true);
    h.add_game("first", "Air");
    h.app.enqueue_auto_link().unwrap();
    let search = h.submitted().unwrap();
    h.complete(search, Ok(MetadataPayload::Search(Vec::new())));
    h.app.enqueue_auto_link().unwrap();
    assert!(h.submitted().is_none());

    let root = h._root.path().to_owned();
    drop(h.app);
    let mut app =
        AstraEmuManagerController::open_library(root, FrameMailbox::new(), Vec::new()).unwrap();
    let (runtime, commands, _completions) = MetadataRuntime::scripted();
    app.metadata = runtime;
    app.network_allowed = true;
    assert!(app.auto_link_attempted.contains("first"));
    app.enqueue_auto_link().unwrap();
    assert!(commands.try_recv().is_err());
}

#[test]
fn unlinked_games_are_not_linked_again() {
    let mut h = harness(true);
    h.add_game("first", "Air");
    h.app.selected_case_id = Some("first".into());
    h.app
        .apply_metadata_payload(
            Some("first"),
            MetadataProviderId::Vndb,
            MetadataPayload::Fetch {
                record: Box::new(record("v1", "AIR", &[])),
                cover: None,
            },
        )
        .unwrap();
    h.app.unlink_identity("vndb").unwrap();
    assert!(h.vndb_remote_id("first").is_none());
    h.app.enqueue_auto_link().unwrap();
    assert!(h.submitted().is_none());
}

#[test]
fn request_error_stops_the_batch_without_marking() {
    let mut h = harness(true);
    h.add_game("a", "Air");
    h.add_game("b", "Kanon");
    h.app.enqueue_auto_link().unwrap();
    let first = h.submitted().unwrap();
    h.complete(first, Err("ASTRA_EMU_METADATA_RATE_LIMITED".into()));
    assert!(h.submitted().is_none());
    assert!(h.app.auto_link_queue.is_empty());
    assert!(h.app.auto_link_attempted.is_empty());
    assert_eq!(h.app.diagnostic, "ASTRA_EMU_METADATA_RATE_LIMITED");

    h.app.enqueue_auto_link().unwrap();
    let retried = h.submitted().unwrap();
    assert_eq!(search_title(&retried), "Air");
}

#[test]
fn manual_request_removes_a_waiting_game_from_the_queue() {
    let mut h = harness(true);
    h.add_game("a", "Air");
    h.add_game("b", "Kanon");
    h.app.enqueue_auto_link().unwrap();
    let first = h.submitted().unwrap();
    assert_eq!(h.app.auto_link_queue, ["b"]);
    h.app.selected_case_id = Some("b".into());
    h.app.search_metadata("vndb", "Kanon").unwrap();
    assert!(h.app.auto_link_queue.is_empty());
    let manual = h.submitted().unwrap();
    h.complete(first, Ok(MetadataPayload::Search(Vec::new())));
    assert!(h.submitted().is_none());
    h.complete(
        manual,
        Ok(MetadataPayload::Search(vec![record("v3", "Kanon", &[])])),
    );
    assert!(h.vndb_remote_id("b").is_none());
    assert_eq!(h.status("b"), "找到 1 个作品，请选择关联");
    assert!(!h.app.auto_link_attempted.contains("b"));
}

#[test]
fn stored_vndb_descriptions_are_shown_without_formatting() {
    let mut h = harness(false);
    h.add_game("first", "Air");
    let mut linked = record("v1", "AIR", &[]);
    linked.description = Some("[b]Summer[/b] story.\n[spoiler]Secret[/spoiler]".into());
    h.app
        .link_metadata("first", MetadataProviderId::Vndb, linked, None)
        .unwrap();
    let shown = h
        .app
        .metadata_record("first", MetadataProviderId::Vndb)
        .unwrap()
        .unwrap();
    assert_eq!(shown.description.as_deref(), Some("Summer story."));
}
