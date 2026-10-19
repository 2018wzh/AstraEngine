use crate::CoreError;
use astra_media_core::{
    BlendMode, GlyphBitmap, GlyphBitmapFormat, GlyphInstance, RectI, SceneCommand,
};
use astra_subtitle::{AssRenderer, FontData, FrameChange, VideoColorSpace};
use std::sync::Arc;

/// Owner-thread ASS renderer with retained GPU masks. No video pixels pass through it.
pub struct AssScene {
    renderer: AssRenderer,
    namespace: String,
    masks: Vec<(String, u32, u32)>,
    draws: Vec<SceneCommand>,
    layout: Option<((u32, u32), RectI, VideoColorSpace)>,
    revision: u64,
}
impl AssScene {
    pub fn new(namespace: String, ass_utf8: &[u8], fonts: &[FontData]) -> Result<Self, CoreError> {
        if namespace.is_empty() || namespace.len() > 256 {
            return Err(failed());
        }
        Ok(Self {
            renderer: AssRenderer::new(ass_utf8, fonts).map_err(native_error)?,
            namespace,
            masks: Vec::new(),
            draws: Vec::new(),
            layout: None,
            revision: 0,
        })
    }

    /// Append only resource mutations. Unchanged frames allocate and upload nothing.
    /// Positions (native 1) reuse masks; Content (native 2) replaces them.
    pub fn update(
        &mut self,
        position_us: u64,
        storage: (u32, u32),
        viewport: RectI,
        video: VideoColorSpace,
        mutations: &mut Vec<SceneCommand>,
    ) -> Result<bool, CoreError> {
        if viewport.x < 0 || viewport.y < 0 || viewport.width == 0 || viewport.height == 0 {
            return Err(failed());
        }
        let layout = (storage, viewport, video);
        let frame = self
            .renderer
            .render(
                position_us / 1000,
                storage,
                (viewport.width, viewport.height),
            )
            .map_err(native_error)?;
        if frame.change == FrameChange::Unchanged && self.layout == Some(layout) {
            return Ok(false);
        }
        let content = frame.change == FrameChange::Content || self.layout.is_none();
        if !content && self.masks.len() != frame.runs.len() {
            return Err(failed());
        }
        if content {
            self.revision = self.revision.checked_add(1).ok_or_else(failed)?;
            for (resource_id, _, _) in self.masks.drain(..) {
                mutations.push(SceneCommand::ReleaseResource { resource_id });
            }
            self.draws.clear();
        }
        let mut mask_bytes = 0usize;
        for (index, run) in frame.runs.iter().enumerate() {
            let x = i32::try_from(run.x)
                .ok()
                .and_then(|x| viewport.x.checked_add(x))
                .ok_or_else(failed)?;
            let y = i32::try_from(run.y)
                .ok()
                .and_then(|y| viewport.y.checked_add(y))
                .ok_or_else(failed)?;
            let rgba = run
                .rgba_for_video(frame.matrix, video)
                .map_err(native_error)?;
            if content {
                let size = (run.w as usize)
                    .checked_mul(run.h as usize)
                    .ok_or_else(failed)?;
                mask_bytes = mask_bytes
                    .checked_add(size)
                    .filter(|size| *size <= 64 * 1024 * 1024)
                    .ok_or_else(failed)?;
                let mut pixels = Vec::with_capacity(size);
                for row in 0..run.h as usize {
                    let start = row.checked_mul(run.stride).ok_or_else(failed)?;
                    pixels.extend_from_slice(
                        run.alpha
                            .get(start..start + run.w as usize)
                            .ok_or_else(failed)?,
                    );
                }
                let resource_id = format!("{}/{}/{}", self.namespace, self.revision, index);
                mutations.push(SceneCommand::UploadGlyph {
                    resource_id: resource_id.clone(),
                    glyph: GlyphBitmap::from_vec(run.w, run.h, GlyphBitmapFormat::Alpha8, pixels)
                        .map_err(|_| failed())?,
                });
                self.draws.push(SceneCommand::GlyphRun {
                    id: resource_id.clone(),
                    glyphs: Arc::from([GlyphInstance {
                        resource_id: resource_id.clone(),
                        x,
                        y,
                        rotation_quadrants: 0,
                    }]),
                    rgba,
                    opacity: 1.0,
                    blend: BlendMode::Alpha,
                });
                self.masks.push((resource_id, run.w, run.h));
            } else {
                if (self.masks[index].1, self.masks[index].2) != (run.w, run.h) {
                    return Err(failed());
                }
                let SceneCommand::GlyphRun {
                    glyphs,
                    rgba: color,
                    ..
                } = &mut self.draws[index]
                else {
                    unreachable!()
                };
                let glyph = &mut Arc::make_mut(glyphs)[0];
                glyph.x = x;
                glyph.y = y;
                *color = rgba;
            }
        }
        self.layout = Some(layout);
        Ok(true)
    }

    pub fn append_draws(&self, commands: &mut Vec<SceneCommand>) {
        commands.extend(self.draws.iter().cloned());
    }

    /// Release residency before a track is replaced or its owner is dropped.
    pub fn release_resources(&mut self, commands: &mut Vec<SceneCommand>) {
        for (resource_id, _, _) in self.masks.drain(..) {
            commands.push(SceneCommand::ReleaseResource { resource_id });
        }
        self.draws.clear();
        self.layout = None;
    }
}
fn failed() -> CoreError {
    CoreError::invalid(
        "ASTRA_EMU_SUBTITLE_SCENE",
        "subtitle geometry or mask residency is invalid",
    )
}
fn native_error(error: astra_subtitle::SubtitleError) -> CoreError {
    let code = match error.kind {
        astra_subtitle::ErrorKind::MissingFont => "ASTRA_EMU_SUBTITLE_FONT",
        astra_subtitle::ErrorKind::MissingGlyph => "ASTRA_EMU_SUBTITLE_GLYPH",
        astra_subtitle::ErrorKind::ColorSpace => "ASTRA_EMU_SUBTITLE_COLOR",
        _ => "ASTRA_EMU_SUBTITLE_RENDER",
    };
    CoreError::invalid(code, "native subtitle rendering failed")
}
