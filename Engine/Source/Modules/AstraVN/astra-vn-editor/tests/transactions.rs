use astra_vn_editor::*;

const SOURCE: &str = "# editor comment\nstory main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:hello speaker:narrator #@id hello\n";

fn workspace() -> AuthoringWorkspace {
    let mut workspace = AuthoringWorkspace::default();
    workspace
        .open(AstraSource::story("main.astra", SOURCE))
        .unwrap();
    workspace
}

#[test]
fn inspector_edit_compiles_preserves_comments_and_undo_uses_new_version() {
    let mut w = workspace();
    let batch = w
        .attribute_edit("main.astra", "hello", "speaker", "Alice")
        .unwrap();
    w.apply(batch.clone()).unwrap();
    assert!(w
        .document("main.astra")
        .unwrap()
        .text
        .starts_with("# editor comment\n"));
    let compiled = w.compile(Default::default()).unwrap();
    assert!(compiled.story.source_map.contains_key("hello"));
    w.undo().unwrap();
    assert_eq!(w.document("main.astra").unwrap().text, SOURCE);
    assert_eq!(w.document("main.astra").unwrap().version, 3);
    assert_eq!(w.apply(batch), Err(EditError::Conflict));
    assert!(!w.is_dirty("main.astra").unwrap());
    w.redo().unwrap();
    assert!(w.is_dirty("main.astra").unwrap());
}

#[test]
fn batch_rejects_partial_conflicts_and_unicode_splits() {
    let mut w = workspace();
    w.open(AstraSource::story("second.astra", "你好")).unwrap();
    let good = w
        .attribute_edit("main.astra", "hello", "speaker", "Alice")
        .unwrap();
    let mut batch = good;
    batch.documents.push(DocumentEdits {
        path: "second.astra".into(),
        version: 1,
        edits: vec![TextEdit {
            start: 1,
            end: 2,
            replacement: "x".into(),
        }],
    });
    assert_eq!(w.apply(batch), Err(EditError::InvalidRange));
    assert_eq!(w.document("main.astra").unwrap().text, SOURCE);
    assert_eq!(w.undo(), Err(EditError::NoHistory));
}

#[test]
fn overlapping_edits_and_unsafe_paths_fail() {
    let mut w = workspace();
    for path in [
        "../outside.astra",
        "/root.astra",
        "C:/root.astra",
        "a\\b.astra",
    ] {
        assert_eq!(
            w.open(AstraSource::story(path, "")),
            Err(EditError::InvalidPath)
        );
    }
    let edit = TextEdit {
        start: 0,
        end: 0,
        replacement: "# one\n".into(),
    };
    assert_eq!(
        w.apply(EditBatch {
            documents: vec![DocumentEdits {
                path: "main.astra".into(),
                version: 1,
                edits: vec![edit.clone(), edit],
            }]
        }),
        Err(EditError::OverlappingEdits)
    );
}
