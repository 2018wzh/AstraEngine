use astra_vn_editor::{parse_astra_source, AuthoringWorkspace, DocumentSnapshot, EditBatch};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keyframe {
    pub time_ms: u32,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct TimelineTrack {
    pub source_id: String,
    pub target: String,
    pub property: String,
    pub frames: Vec<Keyframe>,
}

/// A disposable projection of the canonical source, never a second timeline file.
pub fn tracks(document: &DocumentSnapshot) -> anyhow::Result<Vec<TimelineTrack>> {
    let parsed = parse_astra_source(&document.path, &document.text);
    let mut tracks = Vec::new();
    for command in parsed
        .ast
        .commands()
        .filter(|command| command.keyword() == "timeline")
    {
        let Some(encoded) = command
            .attributes()
            .find(|attribute| attribute.key() == "keyframes")
        else {
            continue;
        };
        let source_id = command
            .source_id()
            .ok_or_else(|| anyhow::anyhow!("Timeline needs a stable source ID"))?;
        let frames = encoded
            .value()
            .split(',')
            .map(|pair| {
                let (time, value) = pair
                    .split_once('=')
                    .ok_or_else(|| anyhow::anyhow!("Invalid keyframe in {source_id}"))?;
                Ok(Keyframe {
                    time_ms: time.parse()?,
                    value: value.into(),
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        validate(&frames)?;
        let attribute = |key| {
            command
                .attributes()
                .find(|attribute| attribute.key() == key)
                .map(|attribute| attribute.value().to_string())
                .unwrap_or_default()
        };
        tracks.push(TimelineTrack {
            source_id: source_id.into(),
            target: attribute("target"),
            property: attribute("property"),
            frames,
        });
    }
    Ok(tracks)
}

fn validate(frames: &[Keyframe]) -> anyhow::Result<()> {
    anyhow::ensure!(frames.len() >= 2, "A timeline needs at least two keyframes");
    anyhow::ensure!(
        frames
            .windows(2)
            .all(|pair| pair[0].time_ms < pair[1].time_ms),
        "Keyframe times must be unique and increasing"
    );
    anyhow::ensure!(
        frames
            .iter()
            .all(|frame| !frame.value.is_empty() && !frame.value.contains([',', '=', '\n', '\r'])),
        "A keyframe must contain one scalar value"
    );
    Ok(())
}

pub enum KeyframeEdit {
    Set { index: usize, frame: Keyframe },
    Insert(Keyframe),
    Remove { index: usize },
}

pub fn edit(
    workspace: &AuthoringWorkspace,
    path: &str,
    version: u64,
    source_id: &str,
    edit: KeyframeEdit,
) -> anyhow::Result<EditBatch> {
    let document = workspace.document(path)?;
    anyhow::ensure!(
        document.version == version,
        "Timeline changed; select the keyframe again"
    );
    let mut track = tracks(document)?
        .into_iter()
        .find(|track| track.source_id == source_id)
        .ok_or_else(|| anyhow::anyhow!("Timeline source command no longer exists"))?;
    match edit {
        KeyframeEdit::Set { index, frame } => {
            *track
                .frames
                .get_mut(index)
                .ok_or_else(|| anyhow::anyhow!("Keyframe no longer exists"))? = frame;
        }
        KeyframeEdit::Insert(frame) => track.frames.push(frame),
        KeyframeEdit::Remove { index } => {
            anyhow::ensure!(index < track.frames.len(), "Keyframe no longer exists");
            track.frames.remove(index);
        }
    }
    track.frames.sort_by_key(|frame| frame.time_ms);
    validate(&track.frames)?;
    let value = track
        .frames
        .iter()
        .map(|frame| format!("{}={}", frame.time_ms, frame.value))
        .collect::<Vec<_>>()
        .join(",");
    Ok(workspace.attribute_edit(path, source_id, "keyframes", &value)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_vn_editor::AstraSource;

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
