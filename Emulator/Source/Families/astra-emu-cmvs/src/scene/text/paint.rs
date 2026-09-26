use super::*;
use crate::CmvsNativeTextGlyph;
use astra_media_core::{
    GlyphBitmap, GlyphBitmapFormat, GlyphInstance, MeshMaterial2D, MeshVertex2D, TextureFilter2D,
};

#[derive(Default)]
pub(super) struct NativePaint {
    dimensions: BTreeMap<String, [u32; 2]>,
}

impl NativePaint {
    pub(super) fn project(
        &mut self,
        commands: Vec<SceneCommand>,
        styles: &BTreeMap<String, (&CmvsNativeTextGlyph, [i32; 2])>,
    ) -> Result<Vec<SceneCommand>, CoreError> {
        let mut output = Vec::new();
        for command in commands {
            match command {
                SceneCommand::UploadGlyph { resource_id, glyph } => {
                    for radius in 1..=2 {
                        output.push(SceneCommand::UploadGlyph {
                            resource_id: outline_id(&resource_id, radius),
                            glyph: outline(&glyph, radius)?,
                        });
                    }
                    self.dimensions
                        .insert(resource_id.clone(), [glyph.width, glyph.height]);
                    output.push(SceneCommand::UploadGlyph { resource_id, glyph });
                }
                SceneCommand::ReleaseResource { resource_id } => {
                    self.dimensions.remove(&resource_id);
                    for radius in 1..=2 {
                        output.push(SceneCommand::ReleaseResource {
                            resource_id: outline_id(&resource_id, radius),
                        });
                    }
                    output.push(SceneCommand::ReleaseResource { resource_id });
                }
                SceneCommand::GlyphRun { id, glyphs, .. } => {
                    let (style, origin) = styles
                        .get(&id)
                        .ok_or_else(|| error("ASTRA_EMU_CMVS_TEXT_STYLE"))?;
                    if style.style > 2 {
                        return Err(error("ASTRA_EMU_CMVS_TEXT_STYLE"));
                    }
                    for (index, glyph) in glyphs.iter().enumerate() {
                        if glyph.rotation_quadrants != 0 {
                            return Err(error("ASTRA_EMU_CMVS_TEXT_ROTATION"));
                        }
                        let dimensions = *self
                            .dimensions
                            .get(&glyph.resource_id)
                            .ok_or_else(|| error("ASTRA_EMU_CMVS_GLYPH_RESOURCE"))?;
                        if style.style != 0 {
                            let radius = style.style;
                            output.push(SceneCommand::GlyphRun {
                                id: format!("{id}.{index}.outline"),
                                glyphs: vec![GlyphInstance {
                                    resource_id: outline_id(&glyph.resource_id, radius),
                                    x: glyph
                                        .x
                                        .checked_sub(radius as i32)
                                        .ok_or_else(|| error("ASTRA_EMU_CMVS_TEXT_GEOMETRY"))?,
                                    y: glyph
                                        .y
                                        .checked_sub(radius as i32)
                                        .ok_or_else(|| error("ASTRA_EMU_CMVS_TEXT_GEOMETRY"))?,
                                    rotation_quadrants: 0,
                                }]
                                .into(),
                                rgba: [0, 0, 0, 255],
                                opacity: 1.0,
                                blend: BlendMode::Alpha,
                            });
                        }
                        let [width, height] = dimensions;
                        let vertices = [
                            [0.0, 0.0],
                            [width as f32, 0.0],
                            [width as f32, height as f32],
                            [0.0, height as f32],
                        ]
                        .into_iter()
                        .zip([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]])
                        .map(|(delta, uv)| {
                            let position = [glyph.x as f32 + delta[0], glyph.y as f32 + delta[1]];
                            MeshVertex2D {
                                position,
                                uv,
                                premultiplied_rgba: color(
                                    style,
                                    [
                                        position[0] - origin[0] as f32,
                                        position[1] - origin[1] as f32,
                                    ],
                                ),
                            }
                        })
                        .collect::<Vec<_>>();
                        output.push(SceneCommand::Mesh2D {
                            id: format!("{id}.{index}"),
                            vertices: vertices.into(),
                            indices: vec![0, 1, 2, 0, 2, 3].into(),
                            material: MeshMaterial2D::GlyphMask,
                            texture_id: Some(glyph.resource_id.clone()),
                            texture_filter: TextureFilter2D::Nearest,
                            opacity: 1.0,
                            blend: BlendMode::Alpha,
                        });
                    }
                }
                _ => return Err(error("ASTRA_EMU_CMVS_TEXT_COMMAND")),
            }
        }
        Ok(output)
    }
}

fn outline_id(id: &str, radius: u32) -> String {
    format!("{id}.cmvs-outline.{radius}")
}

// Native styles 1/2 use a maximum-alpha diamond kernel, including attenuated
// corners. Repeated source-over glyph draws do not implement that operation.
fn outline(glyph: &GlyphBitmap, radius: u32) -> Result<GlyphBitmap, CoreError> {
    if glyph.format != GlyphBitmapFormat::Alpha8 {
        return Err(error("ASTRA_EMU_CMVS_GLYPH_FORMAT"));
    }
    let width = glyph.width + radius * 2;
    let height = glyph.height + radius * 2;
    let mut pixels = vec![0u8; width as usize * height as usize];
    for y in 0..glyph.height {
        for x in 0..glyph.width {
            let alpha = glyph.pixels.as_slice()[(y * glyph.width + x) as usize];
            for dy in 0..=radius * 2 {
                for dx in 0..=radius * 2 {
                    let distance = dx.abs_diff(radius) + dy.abs_diff(radius);
                    let value = alpha >> distance.saturating_sub(radius);
                    let target = &mut pixels[((y + dy) * width + x + dx) as usize];
                    *target = (*target).max(value);
                }
            }
        }
    }
    GlyphBitmap::from_vec(width, height, GlyphBitmapFormat::Alpha8, pixels)
        .map_err(|_| error("ASTRA_EMU_CMVS_GLYPH_FORMAT"))
}

fn color(style: &CmvsNativeTextGlyph, point: [f32; 2]) -> [u8; 4] {
    let distance = match style.color_mode {
        1 => point[1],
        2 => point[0],
        3 => (point[0] + point[1]) * 0.5,
        _ => 0.0,
    };
    let fraction = (distance / style.size as f32).clamp(0.0, 1.0);
    let mut rgba = [255; 4];
    for (channel, shift) in rgba[..3].iter_mut().zip([16, 8, 0]) {
        let start = ((style.colors[0] >> shift) & 255) as f32;
        let end = ((style.colors[1] >> shift) & 255) as f32;
        *channel = (start + (end - start) * fraction) as u8;
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_outline_uses_maximum_coverage_and_attenuated_corners() {
        let source = GlyphBitmap::from_vec(1, 1, GlyphBitmapFormat::Alpha8, vec![200]).unwrap();
        let outline = outline(&source, 2).unwrap();
        assert_eq!(
            outline.pixels.as_slice(),
            &[
                50, 100, 200, 100, 50, 100, 200, 200, 200, 100, 200, 200, 200, 200, 200, 100, 200,
                200, 200, 100, 50, 100, 200, 100, 50
            ]
        );
    }

    #[test]
    fn native_color_modes_select_vertical_horizontal_and_diagonal_axes() {
        let mut glyph = CmvsNativeTextGlyph {
            text: "A".into(),
            position: [0, 0],
            font: "test".into(),
            size: 32,
            colors: [0xff0000, 0x0000ff],
            style: 0,
            color_mode: 0,
        };
        assert_eq!(color(&glyph, [16.0, 16.0]), [255, 0, 0, 255]);
        glyph.color_mode = 1;
        assert_eq!(color(&glyph, [32.0, 0.0]), [255, 0, 0, 255]);
        assert_eq!(color(&glyph, [0.0, 32.0]), [0, 0, 255, 255]);
        glyph.color_mode = 2;
        assert_eq!(color(&glyph, [32.0, 0.0]), [0, 0, 255, 255]);
        glyph.color_mode = 3;
        assert_eq!(color(&glyph, [16.0, 16.0]), [127, 0, 127, 255]);
    }
}
