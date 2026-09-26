#[cfg(test)]
use astra_vn_editor::{
    timeline::{edit, tracks, Keyframe, KeyframeEdit},
    AuthoringWorkspace,
};

/// Convert a drag on the 520-pixel time axis. Only release commits one undo batch.
pub fn drag_time(original_ms: u32, delta_pixels: f32, duration_ms: u32) -> u32 {
    (original_ms as f64 + f64::from(delta_pixels) / 520. * f64::from(duration_ms))
        .round()
        .clamp(0., f64::from(u32::MAX)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_vn_editor::AstraSource;

    #[test]
    fn dragging_is_bounded() {
        assert_eq!(drag_time(100, 260., 1000), 600);
        assert_eq!(drag_time(100, -520., 1000), 0);
        assert_eq!(drag_time(u32::MAX, 520., 1000), u32::MAX);
    }
    #[test]
    fn a_drag_commits_once_and_undo_restores_the_complete_gesture() {
        let source = "story main #@id main\nstate start #@id start\n  scene room #@id room\n    timeline action:start id:pan target:hero property:x keyframes:0=0,1000=1 budget_ms:2000 #@id pan\n";
        let mut workspace = AuthoringWorkspace::default();
        workspace
            .open(AstraSource::story("main.astra", source))
            .unwrap();
        let mut time_ms = 0;
        for motion in 0..=260 {
            time_ms = drag_time(1000, motion as f32, 1000);
        }
        assert_eq!(workspace.document("main.astra").unwrap().version, 1);
        workspace
            .apply(
                edit(
                    &workspace,
                    "main.astra",
                    1,
                    "pan",
                    KeyframeEdit::Set {
                        index: 1,
                        frame: Keyframe {
                            time_ms,
                            value: "1".into(),
                        },
                    },
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(workspace.document("main.astra").unwrap().version, 2);
        assert_eq!(
            tracks(workspace.document("main.astra").unwrap()).unwrap()[0].frames[1].time_ms,
            1500
        );
        workspace.undo().unwrap();
        assert_eq!(workspace.document("main.astra").unwrap().text, source);
        assert!(workspace.undo().is_err());
    }

    #[test]
    fn keyframe_edit_preserves_comment_and_undo_rejects_stale_selection() {
        let source = "# author note\nstory main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    timeline action:start id:pan target:hero property:x keyframes:0=0,1000=1 budget_ms:2000 #@id pan\n";
        let mut workspace = AuthoringWorkspace::default();
        workspace
            .open(AstraSource::story("main.astra", source))
            .unwrap();
        let batch = edit(
            &workspace,
            "main.astra",
            1,
            "pan",
            KeyframeEdit::Insert(Keyframe {
                time_ms: 500,
                value: "0.5".into(),
            }),
        )
        .unwrap();
        workspace.apply(batch).unwrap();
        let document = workspace.document("main.astra").unwrap();
        assert!(document.text.starts_with("# author note\n"));
        assert!(document.text.ends_with("#@id pan\n"));
        assert_eq!(tracks(document).unwrap()[0].frames[1].time_ms, 500);
        let compiled = workspace.compile(Default::default()).unwrap();
        assert_eq!(compiled.story.source_map.get("pan").unwrap().line, 5);
        workspace.undo().unwrap();
        assert_eq!(workspace.document("main.astra").unwrap().text, source);
        assert!(edit(
            &workspace,
            "main.astra",
            1,
            "pan",
            KeyframeEdit::Remove { index: 0 }
        )
        .is_err());
        let version = workspace.document("main.astra").unwrap().version;
        assert!(edit(
            &workspace,
            "main.astra",
            version,
            "pan",
            KeyframeEdit::Remove { index: 0 }
        )
        .is_err());
        assert!(edit(
            &workspace,
            "main.astra",
            version,
            "pan",
            KeyframeEdit::Insert(Keyframe {
                time_ms: 1000,
                value: "2".into()
            })
        )
        .is_err());
    }
}
