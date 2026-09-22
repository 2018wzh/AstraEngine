use super::*;
use astra_vn_core::PreviewDocumentRevision;

fn source() -> NativeVnHostCommandSource {
    let bytes = crate::test_native_package::product_package_with_request(
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:line.one speaker:hero #@id line.one\n", |_| {});
    let package = astra_package::PackageReader::open(&bytes).unwrap();
    let mut source = NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap();
    source.launch().unwrap();
    source
}
fn identity(source: &NativeVnHostCommandSource) -> PreviewIdentity {
    PreviewIdentity {
        project_hash: source.compiled_project_hash,
        generation: 1,
        documents: [(
            "Scripts/main.astra".into(),
            PreviewDocumentRevision {
                version: 1,
                content_hash: Hash256::from_sha256(b"document"),
            },
        )]
        .into(),
    }
}
fn executor() -> astra_player_core::PlayerHostCommandExecutor<astra_player_core::PlatformCommandSink>
{
    let (client, _, _) = astra_platform::host_channel(
        astra_platform::PlatformHostProfile::windows_release("nativevn-game", "com.example.player"),
        1,
        1,
    )
    .unwrap();
    astra_player_core::PlayerHostCommandExecutor::new(astra_player_core::PlatformCommandSink::new(
        client,
    ))
}
#[test]
fn rejects_wrong_project_revisions_duplicates_and_cancelled_session() {
    let mut source = source();
    let id = identity(&source);
    let mut invalid = id.clone();
    invalid.project_hash = Hash256::from_sha256(b"other");
    assert!(matches!(
        NativeVnPreview::attach(&source, invalid),
        Err(Reject::StaleIdentity)
    ));
    let mut preview = NativeVnPreview::attach(&source, id.clone()).unwrap();
    preview.accept(&id, 1).unwrap();
    assert_eq!(preview.accept(&id, 1), Err(Reject::StaleRequest));
    let mut newer = id.clone();
    newer.documents.values_mut().next().unwrap().version += 1;
    assert_eq!(preview.accept(&newer, 2), Err(Reject::StaleIdentity));
    preview.accept(&id, 2).unwrap();
    source.reset_pending_work();
    assert_eq!(preview.accept(&id, 3), Err(Reject::StaleIdentity));
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}
#[tokio::test]
async fn exact_fragment_seek_restores_without_executing_story_and_cancels_old_work() {
    let mut source = source();
    let mut media = NativeVnProductMediaHost::default();
    let mut executor = executor();
    let mut preview = NativeVnPreview::attach(&source, identity(&source)).unwrap();
    preview.record(&mut source, &media).unwrap();
    let original = preview.status().checkpoints[0].clone();
    let step = source.fixed_step;
    let backlog = source.runtime_state.as_ref().unwrap().backlog.clone();
    source.tick_presentation(250_000_000).unwrap();
    preview.record(&mut source, &media).unwrap();
    assert!(preview.status().checkpoints.len() > 1);
    preview
        .set_paused(true, &mut media, &executor)
        .await
        .unwrap();
    let scope = source.media_scope.child();
    preview
        .seek(
            "line.one",
            original.id,
            &mut source,
            &mut media,
            &mut executor,
        )
        .await
        .unwrap();
    assert_eq!(
        source.stage_director.state().elapsed_ns,
        original.presentation_time_ns
    );
    assert_eq!(
        preview.status().presentation_time_ns,
        original.presentation_time_ns
    );
    assert_eq!(source.fixed_step, step);
    assert_eq!(source.runtime_state.as_ref().unwrap().backlog, backlog);
    assert!(scope.is_cancelled());
    assert!(!preview.source_scope.is_cancelled());
    assert!(preview.is_paused());
    preview
        .set_paused(false, &mut media, &executor)
        .await
        .unwrap();
    source.tick_presentation(16_666_667).unwrap();
    assert_eq!(
        source.stage_director.state().elapsed_ns,
        original.presentation_time_ns + 16_666_667
    );
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}
#[tokio::test]
async fn invalid_or_corrupt_checkpoint_preserves_live_state_and_work() {
    let mut source = source();
    let mut media = NativeVnProductMediaHost::default();
    let mut executor = executor();
    let mut preview = NativeVnPreview::attach(&source, identity(&source)).unwrap();
    preview.record(&mut source, &media).unwrap();
    let checkpoint = preview.status().checkpoints[0].id;
    assert!(matches!(
        preview
            .seek(
                "line.one",
                checkpoint,
                &mut source,
                &mut media,
                &mut executor
            )
            .await,
        Err(Reject::NotPaused)
    ));
    preview
        .set_paused(true, &mut media, &executor)
        .await
        .unwrap();
    assert!(matches!(
        preview
            .seek("other", checkpoint, &mut source, &mut media, &mut executor)
            .await,
        Err(Reject::CrossFragment)
    ));
    assert!(matches!(
        preview
            .seek("line.one", u64::MAX, &mut source, &mut media, &mut executor)
            .await,
        Err(Reject::CheckpointUnavailable)
    ));
    let before = source.host.save().unwrap();
    let scope = source.media_scope.child();
    preview.checkpoints[0].state.runtime.0.clear();
    assert!(matches!(
        preview
            .seek(
                "line.one",
                checkpoint,
                &mut source,
                &mut media,
                &mut executor
            )
            .await,
        Err(Reject::RestoreFailed)
    ));
    assert_eq!(source.host.save().unwrap(), before);
    assert!(!scope.is_cancelled());
    preview.cancel();
    assert!(matches!(
        preview
            .seek(
                "line.one",
                checkpoint,
                &mut source,
                &mut media,
                &mut executor
            )
            .await,
        Err(Reject::StaleIdentity)
    ));
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}
