use std::collections::BTreeSet;

use astra_media_core::SceneCommand;

// Skipped frames may share one GPU submission only while their resource
// mutations are independent. Preserve a real submission boundary before a
// resource is changed again; in particular release/re-upload is not one frame.
pub(super) fn crosses_resource_boundary(
    deferred: &[SceneCommand],
    incoming: &[SceneCommand],
) -> bool {
    if deferred.is_empty() {
        return false;
    }
    let ids = deferred
        .iter()
        .filter_map(resource_id)
        .collect::<BTreeSet<_>>();
    incoming
        .iter()
        .filter_map(resource_id)
        .any(|id| ids.contains(id))
}

fn resource_id(command: &SceneCommand) -> Option<&str> {
    match command {
        SceneCommand::UploadTexture { resource_id, .. }
        | SceneCommand::UploadGlyph { resource_id, .. }
        | SceneCommand::UpdateTextureRegion { resource_id, .. }
        | SceneCommand::ReleaseResource { resource_id } => Some(resource_id),
        _ => None,
    }
}
