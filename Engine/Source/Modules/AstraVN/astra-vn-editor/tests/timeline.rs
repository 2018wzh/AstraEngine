use astra_vn_editor::{timeline::*, *};

#[test]
fn easing_and_keys_share_source_spans_versions_and_one_undo() {
    let source="# author note\nstory main #@id story\nstate start #@id start\n  scene room #@id room\n    timeline id:pan target:hero property:x keyframes:0=0,1000=1 budget_ms:2000 #@id pan\n# tail\n";
    let mut workspace = AuthoringWorkspace::default();
    workspace
        .open(AstraSource::story("main.astra", source))
        .unwrap();
    let mut batch = easing_edit(
        &workspace,
        "main.astra",
        1,
        "pan",
        VnPresentationEasing::EaseInOut,
    )
    .unwrap();
    let keys = edit(
        &workspace,
        "main.astra",
        1,
        "pan",
        KeyframeEdit::Insert(Keyframe {
            time_ms: 500,
            value: "0.25".into(),
        }),
    )
    .unwrap();
    batch.documents[0]
        .edits
        .extend(keys.documents[0].edits.clone());
    workspace.apply(batch).unwrap();
    let doc = workspace.document("main.astra").unwrap();
    assert_eq!(doc.version, 2);
    assert!(doc.text.starts_with("# author note\n"));
    assert!(doc.text.ends_with("#@id pan\n# tail\n"));
    let projection = tracks(doc).unwrap();
    assert_eq!(projection[0].easing, VnPresentationEasing::EaseInOut);
    assert_eq!(projection[0].frames[1].time_ms, 500);
    assert_eq!(
        workspace
            .compile(Default::default())
            .unwrap()
            .story
            .source_map
            .get("pan")
            .unwrap()
            .line,
        5
    );
    assert!(easing_edit(
        &workspace,
        "main.astra",
        1,
        "pan",
        VnPresentationEasing::Linear
    )
    .is_err());
    workspace.undo().unwrap();
    assert_eq!(workspace.document("main.astra").unwrap().text, source);
    assert!(workspace.undo().is_err());
    workspace.redo().unwrap();
    let version = workspace.document("main.astra").unwrap().version;
    workspace
        .apply(
            easing_edit(
                &workspace,
                "main.astra",
                version,
                "pan",
                VnPresentationEasing::EaseOut,
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        tracks(workspace.document("main.astra").unwrap()).unwrap()[0].easing,
        VnPresentationEasing::EaseOut
    );
}

#[test]
fn projection_and_edits_use_cook_scalar_and_easing_validation() {
    let source =
        "timeline id:pan target:hero property:x keyframes:0=0,1000=1 budget_ms:2000 #@id pan\n";
    let mut workspace = AuthoringWorkspace::default();
    workspace
        .open(AstraSource::story("main.astra", source))
        .unwrap();
    for value in ["NaN", "1.0000001", "bad"] {
        assert!(edit(
            &workspace,
            "main.astra",
            1,
            "pan",
            KeyframeEdit::Set {
                index: 1,
                frame: Keyframe {
                    time_ms: 1000,
                    value: value.into()
                }
            }
        )
        .is_err());
    }
    let invalid = DocumentSnapshot {
        path: "main.astra".into(),
        version: 1,
        text: source.replace("timeline ", "timeline easing:unknown "),
    };
    assert!(tracks(&invalid).is_err());
    let duplicate = DocumentSnapshot {
        text: format!("story main #@id pan\n{source}"),
        ..invalid
    };
    assert!(tracks(&duplicate).is_err());
    assert_eq!(workspace.document("main.astra").unwrap().text, source);
}
