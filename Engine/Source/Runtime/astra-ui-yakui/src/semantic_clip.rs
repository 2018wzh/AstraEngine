use astra_ui_core::{UiPoint, UiRect, UiValidationError};
use std::collections::BTreeMap;
use yakui_core::layout::{LayoutDom, LayoutDomNode};

pub(crate) fn append_layout_clip(
    dom: &LayoutDom,
    layout: &LayoutDomNode,
    properties: &mut BTreeMap<String, String>,
) -> Result<(), UiValidationError> {
    let mut ancestor = layout.clipped_by;
    let mut clip: Option<yakui_core::geometry::Rect> = None;
    while let Some(id) = ancestor {
        let node = dom.get(id).ok_or_else(|| {
            UiValidationError::invalid(
                "ASTRA_UI_YAKUI_CLIP_LAYOUT",
                "clipping ancestor has no layout node",
            )
        })?;
        clip = Some(clip.map_or(node.rect, |rect| rect.constrain(node.rect)));
        ancestor = node.clipped_by;
    }
    if let Some(clip) = clip {
        let rect = crate::paint::scene_clip(UiRect {
            min: UiPoint {
                x: clip.pos().x,
                y: clip.pos().y,
            },
            max: UiPoint {
                x: (clip.pos().x + clip.size().x).max(clip.pos().x),
                y: (clip.pos().y + clip.size().y).max(clip.pos().y),
            },
        })?;
        // Retained AstraText shares these layout-owned bounds with the mesh bridge.
        for (name, value) in [
            ("x", rect.x.to_string()),
            ("y", rect.y.to_string()),
            ("width", rect.width.to_string()),
            ("height", rect.height.to_string()),
        ] {
            properties.insert(format!("text.clip.{name}"), value);
        }
    }
    Ok(())
}
