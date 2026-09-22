//! Explicit opt-in: starts a real GPU Player using caller-configured built tools.
use astra_editor::{
    preview::{Preview, PreviewConfig},
    project::Project,
};
use astra_vn_editor::{PreviewCommand, PreviewDocumentRevision, PreviewIdentity};
use std::time::{Duration, Instant};

fn until(preview: &mut Preview, project: &Project, ready: impl Fn(&Preview) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        preview.poll(&project.documents).unwrap();
        assert!(
            !preview.status.starts_with("Preview command rejected"),
            "{}",
            preview.status
        );
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
    exercise_preview(&config, &project, None);
}

fn exercise_preview(config: &PreviewConfig, project: &Project, expected_fragment: Option<&str>) {
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
    let mut preview = Preview::start(config, identity).unwrap();
    until(&mut preview, project, |p| {
        p.live.as_ref().is_some_and(|s| {
            !s.checkpoints.is_empty()
                && expected_fragment.is_none_or(|expected| s.source_id.as_deref() == Some(expected))
        })
    });
    preview.send(PreviewCommand::Pause).unwrap();
    until(&mut preview, project, |p| {
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
    until(&mut preview, project, |p| {
        p.live
            .as_ref()
            .is_some_and(|s| s.presentation_time_ns == checkpoint.presentation_time_ns)
    });
    preview.send(PreviewCommand::Resume).unwrap();
    until(&mut preview, project, |p| {
        p.live.as_ref().is_some_and(|s| !s.paused)
    });
    preview.stop().unwrap();
    until(&mut preview, project, Preview::is_finished);
    assert_eq!(preview.status, "Preview closed");
}

/// Restore only our own edits if a product process assertion unwinds.
struct SourceRestore(Vec<(std::path::PathBuf, String, String)>);
impl Drop for SourceRestore {
    fn drop(&mut self) {
        for (path, original, edited) in &self.0 {
            if std::fs::read_to_string(path).ok().as_ref() == Some(edited) {
                std::fs::write(path, original).expect("restore test-authored source");
            }
        }
    }
}

#[test]
#[ignore = "requires NativeVN ASTRA_EDITOR_PREVIEW_CONFIG; saves temporary source edits and opens a GPU Player"]
fn nativevn_graph_timeline_preview_and_undo() {
    let path = std::env::var_os("ASTRA_EDITOR_PREVIEW_CONFIG").expect("explicit preview config");
    let config: PreviewConfig = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let mut project = Project::open(&config.project).unwrap();
    let (original_hash, originals) = edit_nativevn(&mut project);
    let root = config.project.parent().unwrap();
    let guard = SourceRestore(
        originals
            .iter()
            .map(|document| {
                (
                    root.join(&document.path),
                    document.text.clone(),
                    project
                        .documents
                        .document(&document.path)
                        .unwrap()
                        .text
                        .clone(),
                )
            })
            .collect(),
    );
    project.save_all().unwrap();
    exercise_preview(&config, &project, Some("experience.stage.first"));
    for _ in 0..3 {
        project.documents.undo().unwrap();
    }
    for original in &originals {
        assert_eq!(
            project.documents.document(&original.path).unwrap().text,
            original.text
        );
    }
    assert_eq!(project.compile().unwrap().project_hash, original_hash);
    project.save_all().unwrap();
    assert_eq!(
        Project::open(&config.project)
            .unwrap()
            .compile()
            .unwrap()
            .project_hash,
        original_hash
    );
    drop(guard);
}

fn edit_nativevn(
    project: &mut Project,
) -> (astra_core::Hash256, Vec<astra_vn_editor::DocumentSnapshot>) {
    use astra_editor::{
        graph,
        timeline::{self, KeyframeEdit},
    };
    let original_hash = project.compile().unwrap().project_hash;
    let originals = project.documents.documents().cloned().collect::<Vec<_>>();
    let source = "Scripts/experience.astra";
    let version = project.documents.document(source).unwrap().version;
    // Exercise the same delete/connect transactions used by the Graph. Keep all
    // routes reachable, but enter the layered performance directly for preview.
    project
        .documents
        .apply(
            graph::remove(
                &project.documents,
                source,
                version,
                "experience.bootstrap.read",
            )
            .unwrap(),
        )
        .unwrap();
    let version = project.documents.document(source).unwrap().version;
    project
        .documents
        .apply(
            graph::connect(
                &project.documents,
                source,
                version,
                "state.prologue",
                "experience_stage",
            )
            .unwrap(),
        )
        .unwrap();
    let document = project.documents.document(source).unwrap();
    let track = timeline::tracks(document)
        .unwrap()
        .into_iter()
        .find(|t| t.source_id == "experience.walk.start")
        .unwrap();
    let mut frame = track.frames.last().unwrap().clone();
    frame.time_ms += 500;
    project
        .documents
        .apply(
            timeline::edit(
                &project.documents,
                source,
                document.version,
                &track.source_id,
                KeyframeEdit::Set {
                    index: track.frames.len() - 1,
                    frame,
                },
            )
            .unwrap(),
        )
        .unwrap();
    let edited = project.compile().unwrap();
    assert_ne!(edited.project_hash, original_hash);
    assert!(edited
        .story
        .source_map
        .get("experience.walk.start")
        .is_some());
    (original_hash, originals)
}

#[test]
fn nativevn_graph_timeline_roundtrip() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../Examples/NativeVN/project.yaml");
    let mut project = Project::open(&manifest).unwrap();
    let (original_hash, originals) = edit_nativevn(&mut project);
    for _ in 0..3 {
        project.documents.undo().unwrap();
    }
    assert_eq!(project.compile().unwrap().project_hash, original_hash);
    for original in originals {
        assert_eq!(
            project.documents.document(&original.path).unwrap().text,
            original.text
        );
    }
}
