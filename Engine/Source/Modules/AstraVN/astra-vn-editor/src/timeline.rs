use crate::{
    compile_timeline_command, parse_astra_source, AuthoringWorkspace, DocumentSnapshot, EditBatch,
    TimelineCommand, VnPresentationEasing,
};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Keyframe {
    pub time_ms: u32,
    pub value: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct TimelineTrack {
    pub easing: VnPresentationEasing,
    pub source_id: String,
    pub target: String,
    pub property: String,
    pub frames: Vec<Keyframe>,
}

/// A disposable projection of the canonical source, never a second timeline file.
pub fn tracks(document: &DocumentSnapshot) -> anyhow::Result<Vec<TimelineTrack>> {
    let parsed = parse_astra_source(&document.path, &document.text);
    anyhow::ensure!(
        parsed
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == "ASTRA_VN_UNKNOWN_COMMAND"),
        "Timeline source has syntax diagnostics"
    );
    let mut ids = std::collections::BTreeSet::new();
    anyhow::ensure!(
        parsed
            .ast
            .commands()
            .filter_map(|command| command.source_id())
            .all(|id| ids.insert(id)),
        "Source IDs must be unique before Timeline editing"
    );
    let mut tracks = Vec::new();
    for command in parsed
        .ast
        .commands()
        .filter(|command| command.keyword() == "timeline")
    {
        let TimelineCommand::Start(spec) = compile_timeline_command(&document.path, command)?
        else {
            continue;
        };
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
        tracks.push(TimelineTrack {
            easing: spec.tracks[0].easing,
            source_id: source_id.into(),
            target: spec.tracks[0].target.clone(),
            property: spec.tracks[0].property.clone(),
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
    let batch = workspace.attribute_edit(path, source_id, "keyframes", &value)?;
    let candidate = DocumentSnapshot {
        text: batch.documents[0].apply_to(&document.text)?,
        ..document.clone()
    };
    tracks(&candidate)?;
    Ok(batch)
}

/// Insert or replace the track rule without reformatting the command or trivia.
pub fn easing_edit(
    workspace: &AuthoringWorkspace,
    path: &str,
    version: u64,
    source_id: &str,
    easing: VnPresentationEasing,
) -> anyhow::Result<EditBatch> {
    let document = workspace.document(path)?;
    anyhow::ensure!(
        document.version == version,
        "Timeline changed; select the track again"
    );
    anyhow::ensure!(
        tracks(document)?
            .iter()
            .any(|track| track.source_id == source_id),
        "Timeline source command no longer exists"
    );
    let parsed = parse_astra_source(path, &document.text);
    let command = parsed
        .ast
        .commands()
        .find(|c| c.source_id() == Some(source_id))
        .unwrap();
    if command.attribute("easing").is_some() {
        return Ok(workspace.attribute_edit(path, source_id, "easing", easing.as_str())?);
    }
    let offset = u32::from(command.keyword_span().end) as usize;
    Ok(EditBatch {
        documents: vec![crate::DocumentEdits {
            path: path.into(),
            version,
            edits: vec![crate::TextEdit {
                start: offset,
                end: offset,
                replacement: format!(" easing:{}", easing.as_str()),
            }],
        }],
    })
}
