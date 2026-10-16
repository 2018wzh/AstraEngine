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

#[test]
fn adjacent_unicode_deletions_and_replacement_undo_as_one_batch() {
    let mut workspace = AuthoringWorkspace::default();
    let original = "é🙂漢字-tail";
    workspace
        .open(AstraSource::story("unicode.astra", original))
        .unwrap();
    workspace
        .apply(EditBatch {
            documents: vec![DocumentEdits {
                path: "unicode.astra".into(),
                version: 1,
                edits: vec![
                    TextEdit {
                        start: 0,
                        end: 2,
                        replacement: String::new(),
                    },
                    TextEdit {
                        start: 2,
                        end: 6,
                        replacement: String::new(),
                    },
                    TextEdit {
                        start: 6,
                        end: 9,
                        replacement: "文".into(),
                    },
                ],
            }],
        })
        .unwrap();
    assert_eq!(
        workspace.document("unicode.astra").unwrap().text,
        "文字-tail"
    );
    workspace.undo().unwrap();
    assert_eq!(workspace.document("unicode.astra").unwrap().text, original);
    workspace.redo().unwrap();
    assert_eq!(
        workspace.document("unicode.astra").unwrap().text,
        "文字-tail"
    );
}

#[test]
fn full_replacement_diff_never_splits_shared_utf8_prefix_or_suffix_bytes() {
    for (before, after) in [("é🙂tail", "ê🙃tail"), ("prefix-é", "prefix-ĩ")] {
        let mut workspace = AuthoringWorkspace::default();
        workspace
            .open(AstraSource::story("unicode.astra", before))
            .unwrap();
        workspace
            .apply(EditBatch {
                documents: vec![DocumentEdits {
                    path: "unicode.astra".into(),
                    version: 1,
                    edits: vec![TextEdit {
                        start: 0,
                        end: before.len(),
                        replacement: after.into(),
                    }],
                }],
            })
            .unwrap();
        workspace.undo().unwrap();
        assert_eq!(workspace.document("unicode.astra").unwrap().text, before);
        workspace.redo().unwrap();
        assert_eq!(workspace.document("unicode.astra").unwrap().text, after);
    }
}

#[test]
fn oversized_history_reservation_rejects_the_whole_batch_and_preserves_undo() {
    let mut workspace = AuthoringWorkspace::with_history_budget(512).unwrap();
    let original = "a".repeat(512);
    workspace
        .open(AstraSource::story("a.astra", &original))
        .unwrap();
    workspace
        .open(AstraSource::story("b.astra", &original))
        .unwrap();
    workspace
        .apply(EditBatch {
            documents: vec![DocumentEdits {
                path: "a.astra".into(),
                version: 1,
                edits: vec![TextEdit {
                    start: 0,
                    end: 1,
                    replacement: "x".into(),
                }],
            }],
        })
        .unwrap();
    let before = workspace.document("a.astra").unwrap().clone();
    let reservation = workspace.history_reserved_bytes();
    let error = workspace
        .apply(EditBatch {
            documents: vec![
                DocumentEdits {
                    path: "a.astra".into(),
                    version: before.version,
                    edits: vec![TextEdit {
                        start: 1,
                        end: 2,
                        replacement: "y".into(),
                    }],
                },
                DocumentEdits {
                    path: "b.astra".into(),
                    version: 1,
                    edits: vec![TextEdit {
                        start: 0,
                        end: original.len(),
                        replacement: "z".repeat(512),
                    }],
                },
            ],
        })
        .unwrap_err();
    assert_eq!(error, EditError::HistoryLimit);
    assert_eq!(workspace.document("a.astra").unwrap(), &before);
    assert_eq!(workspace.document("b.astra").unwrap().text, original);
    assert_eq!(workspace.history_reserved_bytes(), reservation);
    workspace.undo().unwrap();
    assert_eq!(workspace.document("a.astra").unwrap().text, original);
    workspace.redo().unwrap();
    assert_eq!(workspace.document("a.astra").unwrap().text, before.text);
}
