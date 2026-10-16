use astra_vn_editor::*;
use std::{
    sync::{mpsc::RecvTimeoutError, Arc},
    time::Duration,
};

const STORY: &str = "story main #@id story\nstate start #@id start\n  scene room #@id room\n    text key:hello #@id line\n";

fn workspace() -> AuthoringWorkspace {
    let mut workspace = AuthoringWorkspace::default();
    workspace
        .open(AstraSource::story("main.astra", STORY))
        .unwrap();
    workspace
}

fn replace(workspace: &mut AuthoringWorkspace, text: &str) {
    let document = workspace.document("main.astra").unwrap();
    workspace
        .apply(EditBatch {
            documents: vec![DocumentEdits {
                path: document.path.clone(),
                version: document.version,
                edits: vec![TextEdit {
                    start: 0,
                    end: document.text.len(),
                    replacement: text.into(),
                }],
            }],
        })
        .unwrap();
}

#[test]
fn compilation_snapshot_retains_its_document_version_after_edit_and_undo() {
    let mut documents = workspace();
    let old = documents.compilation_snapshot();
    replace(
        &mut documents,
        "story main #@id duplicate\nstate start #@id duplicate\n",
    );
    let changed = documents.compilation_snapshot();
    assert!(changed.compile(Default::default()).is_err());
    assert_eq!(
        old.compile(Default::default())
            .unwrap()
            .story
            .source_map
            .get("line")
            .unwrap()
            .line,
        4
    );
    documents.undo().unwrap();
    assert_eq!(documents.document("main.astra").unwrap().text, STORY);
    assert!(changed.compile(Default::default()).is_err());
}

#[test]
fn newest_compilation_response_cannot_contain_an_older_source_result() {
    let mut compiler = BackgroundCompiler::new(Duration::from_millis(10)).unwrap();
    let options = Arc::new(CompileAstraProjectOptions::default());
    let mut documents = workspace();
    let old = compiler
        .submit(documents.compilation_snapshot(), Arc::clone(&options))
        .unwrap();
    replace(
        &mut documents,
        "story main #@id duplicate\nstate start #@id duplicate\n",
    );
    let current = compiler
        .submit(documents.compilation_snapshot(), Arc::clone(&options))
        .unwrap();
    let result = current.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(result.is_err());
    match old.recv_timeout(Duration::from_secs(1)) {
        Ok(result) => assert!(result.is_ok()), // Already in flight before replacement.
        Err(RecvTimeoutError::Disconnected) => {} // Superseded while pending.
        Err(error) => panic!("old request did not settle: {error}"),
    }
    documents.undo().unwrap();
    let restored = compiler
        .submit(documents.compilation_snapshot(), options)
        .unwrap();
    assert!(restored
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .is_ok());
    compiler.close().unwrap();
}

#[test]
fn close_cancels_debounced_work_and_rejects_new_requests() {
    let mut compiler = BackgroundCompiler::new(Duration::from_secs(3600)).unwrap();
    let documents = workspace();
    let options = Arc::new(CompileAstraProjectOptions::default());
    let response = compiler
        .submit(documents.compilation_snapshot(), Arc::clone(&options))
        .unwrap();
    compiler.close().unwrap();
    assert!(matches!(
        response.recv_timeout(Duration::from_secs(1)),
        Err(RecvTimeoutError::Disconnected)
    ));
    assert_eq!(
        compiler
            .submit(documents.compilation_snapshot(), options)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::BrokenPipe
    );
}
