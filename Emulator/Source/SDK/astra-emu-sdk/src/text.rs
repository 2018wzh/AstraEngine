use astra_core::DiagnosticSeverity;
use astra_media_core::{MediaError, SceneCommand};
use astra_text::{
    CosmicTextLayoutProvider, TextLayoutRequest, TextRenderLayoutUpdate, TextRenderResourceOwner,
};
use std::collections::BTreeSet;

/// Circular glyph outline; uses the same shaped glyph resources as the foreground.
#[derive(Clone, Copy)]
pub struct TextOutline {
    pub radius: u8,
    pub rgba: [u8; 4],
}

/// One core-owned text region. Layout policy and fonts remain explicit inputs.
pub struct TextSceneLayout {
    pub request: TextLayoutRequest,
    pub translation: (i32, i32),
    pub rgba: [u8; 4],
    pub outline: Option<TextOutline>,
}

/// Shaping and glyph residency shared by emulator families.
///
/// A frame supplies all visible layouts. Missing layouts release their resources;
/// shared glyphs are updated atomically across regions before any draw commands.
/// Returned commands must be submitted in order to the same renderer. On a render
/// failure the caller must close the session, rather than discard the lifecycle.
pub struct TextScene {
    provider: CosmicTextLayoutProvider,
    resources: TextRenderResourceOwner,
    visible: BTreeSet<String>,
}

impl TextScene {
    pub fn new(provider: CosmicTextLayoutProvider) -> Self {
        Self {
            provider,
            resources: TextRenderResourceOwner::default(),
            visible: BTreeSet::new(),
        }
    }

    pub fn frame(&mut self, regions: &[TextSceneLayout]) -> Result<Vec<SceneCommand>, MediaError> {
        let mut visible = BTreeSet::new();
        let mut layers = Vec::new();
        for (index, region) in regions.iter().enumerate() {
            if region.request.key.trim().is_empty() || !visible.insert(region.request.key.clone()) {
                return Err(MediaError::message("ASTRA_EMU_SDK_TEXT_LAYOUT_ID"));
            }
            if let Some(outline) = region.outline {
                if !(1..=8).contains(&outline.radius) || outline.rgba[3] == 0 {
                    return Err(MediaError::message("ASTRA_EMU_SDK_TEXT_OUTLINE"));
                }
                let radius = i32::from(outline.radius);
                for y in -radius..=radius {
                    for x in -radius..=radius {
                        if (x == 0 && y == 0) || x * x + y * y > radius * radius {
                            continue;
                        }
                        let key = format!("{}.outline.{x}.{y}", region.request.key);
                        if !visible.insert(key.clone()) {
                            return Err(MediaError::message("ASTRA_EMU_SDK_TEXT_LAYOUT_ID"));
                        }
                        let translation = region
                            .translation
                            .0
                            .checked_add(x)
                            .zip(region.translation.1.checked_add(y))
                            .ok_or_else(|| {
                                MediaError::message("ASTRA_EMU_SDK_TEXT_OUTLINE_BOUNDS")
                            })?;
                        layers.push((key, index, translation, outline.rgba));
                    }
                }
            }
            layers.push((
                region.request.key.clone(),
                index,
                region.translation,
                region.rgba,
            ));
        }
        let layouts = regions
            .iter()
            .map(|region| {
                let layout = self.provider.layout_shared(&region.request)?;
                if layout.diagnostics.iter().any(|diagnostic| {
                    matches!(
                        diagnostic.severity,
                        DiagnosticSeverity::Error | DiagnosticSeverity::Blocking
                    )
                }) {
                    return Err(MediaError::Diagnostics(layout.diagnostics.clone()));
                }
                Ok(layout)
            })
            .collect::<Result<Vec<_>, MediaError>>()?;
        let updates = layers
            .iter()
            .map(|(id, index, translation, rgba)| TextRenderLayoutUpdate {
                layout_id: id,
                layout: &layouts[*index],
                shared_layout: Some(&layouts[*index]),
                rgba: *rgba,
                translation: *translation,
            })
            .collect::<Vec<_>>();
        let removals = self
            .visible
            .difference(&visible)
            .map(String::as_str)
            .collect::<Vec<_>>();
        let frame = self.resources.update_frame(&updates, &removals)?;
        self.visible = visible;
        let mut commands = frame.lifecycle;
        for layout in frame.layouts {
            commands.extend(layout.commands);
        }
        Ok(commands)
    }

    /// Release all glyphs while the consumer's renderer is still alive.
    pub fn clear(&mut self) -> Vec<SceneCommand> {
        self.visible.clear();
        self.resources.shutdown()
    }
}
