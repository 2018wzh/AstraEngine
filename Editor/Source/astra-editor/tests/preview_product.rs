//! Explicit opt-in: starts a real GPU Player using caller-configured built tools.
use astra_editor::{
    preview::{Preview, PreviewConfig},
    project::Project,
};
use astra_vn_editor::{PreviewCommand, PreviewDocumentRevision, PreviewIdentity};
use std::time::{Duration, Instant};

fn until(preview: &mut Preview, project: &Project, ready: impl Fn(&Preview) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        preview.poll(&project.documents).unwrap();
        if ready(preview) {
            return;
        }
        assert!(!preview.is_finished(), "{}", preview.status);
        assert!(Instant::now() < deadline, "Timed out: {}", preview.status);
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "requires ASTRA_EDITOR_PREVIEW_CONFIG and opens a real GPU window"]
fn real_player_attach_pause_fragment_seek_resume_stop() {
    let path = std::env::var_os("ASTRA_EDITOR_PREVIEW_CONFIG").expect("explicit preview config");
    let config: PreviewConfig = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let project = Project::open(&config.project).unwrap();
    let compiled = project.compile().unwrap();
    let identity = PreviewIdentity {
        project_hash: compiled.project_hash,
        generation: 1,
        documents: project
            .documents
            .documents()
            .map(|document| {
                (
                    document.path.clone(),
                    PreviewDocumentRevision {
                        version: document.version,
                        content_hash: astra_core::Hash256::from_sha256(document.text.as_bytes()),
                    },
                )
            })
            .collect(),
    };
    let mut preview = Preview::start(&config, identity).unwrap();
    until(&mut preview, &project, |p| {
        p.live.as_ref().is_some_and(|s| !s.checkpoints.is_empty())
    });
    preview.send(PreviewCommand::Pause).unwrap();
    until(&mut preview, &project, |p| {
        p.live.as_ref().is_some_and(|s| s.paused)
    });
    let status = preview.live.as_ref().unwrap();
    let source_id = status.source_id.clone().expect("current fragment");
    let checkpoint = status
        .checkpoints
        .first()
        .expect("recorded checkpoint")
        .clone();
    preview
        .send(PreviewCommand::SeekWithinFragment {
            source_id,
            checkpoint: checkpoint.id,
        })
        .unwrap();
    until(&mut preview, &project, |p| {
        p.live
            .as_ref()
            .is_some_and(|s| s.presentation_time_ns == checkpoint.presentation_time_ns)
    });
    preview.send(PreviewCommand::Resume).unwrap();
    until(&mut preview, &project, |p| {
        p.live.as_ref().is_some_and(|s| !s.paused)
    });
    preview.stop().unwrap();
    until(&mut preview, &project, Preview::is_finished);
    assert_eq!(preview.status, "Preview closed");
}
