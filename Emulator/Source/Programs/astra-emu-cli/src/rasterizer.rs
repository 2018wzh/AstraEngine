use std::{collections::BTreeMap, sync::Arc};

use astra_core::Hash256;
use astra_emu_family_api::{
    LegacyBlendMode, LegacyDrawV1, LegacyPreparedSceneCommitV1, LegacyRenderFrameV1,
    LegacySceneResourceOperationV1, LegacySceneResourceStateV1, LegacyTextureFormat,
    LegacyVertexV1,
};
use rayon::prelude::*;

#[derive(Clone)]
struct Texture {
    width: u32,
    height: u32,
    rgba8: Vec<u8>,
}

#[derive(Default)]
pub struct CpuStageRasterizer {
    textures: BTreeMap<u32, Arc<Texture>>,
    scene_resources: LegacySceneResourceStateV1,
    width: u32,
    height: u32,
    rgba8: Vec<u8>,
}

impl CpuStageRasterizer {
    /// Validates and applies one incremental semantic scene transaction. The
    /// CPU reference renderer retains texture storage across frames just like
    /// the GPU stage: a partial upload never requires rebuilding a full frame
    /// DTO or re-uploading unchanged texture bytes.
    pub fn prepare_scene_commit(
        &mut self,
        commit: LegacyPreparedSceneCommitV1,
    ) -> Result<LegacyRenderFrameV1, String> {
        let mut resource_state = if commit.reset_resources {
            LegacySceneResourceStateV1::default()
        } else {
            self.scene_resources.clone()
        };
        let verified = resource_state
            .prepare(commit.packet.clone())
            .map_err(|error| format!("ASTRA_EMU_HEADLESS_SCENE_PREPARE:{}", error.code()))?;
        if verified.next_resources != commit.next_resources {
            return Err("ASTRA_EMU_HEADLESS_SCENE_COMMIT_MISMATCH".into());
        }
        let mut textures = if commit.reset_resources {
            BTreeMap::new()
        } else {
            self.textures.clone()
        };
        for operation in &verified.packet.resources {
            match operation {
                LegacySceneResourceOperationV1::CreateTexture(texture) => {
                    Self::insert_texture_into(
                        &mut textures,
                        texture.texture_id,
                        texture.width,
                        texture.height,
                        texture.format,
                        &texture.pixels,
                        texture.content_hash,
                    )?;
                }
                LegacySceneResourceOperationV1::UpdateTexture(texture) => {
                    Self::update_texture_into(
                        &mut textures,
                        texture.texture_id,
                        texture.x,
                        texture.y,
                        texture.width,
                        texture.height,
                        texture.format,
                        &texture.pixels,
                        texture.content_hash,
                    )?;
                }
                LegacySceneResourceOperationV1::DestroyTexture { texture_id } => {
                    textures.remove(texture_id);
                }
            }
        }
        resource_state.commit(verified.clone());
        self.scene_resources = resource_state;
        self.textures = textures;
        self.width = verified.packet.width;
        self.height = verified.packet.height;
        Ok(LegacyRenderFrameV1 {
            width: verified.packet.width,
            height: verified.packet.height,
            texture_updates: Vec::new(),
            draws: verified.packet.draws,
        })
    }

    pub fn prepare(
        &mut self,
        mut frame: LegacyRenderFrameV1,
    ) -> Result<LegacyRenderFrameV1, String> {
        frame.validate().map_err(|error| error.to_string())?;
        for update in std::mem::take(&mut frame.texture_updates) {
            self.insert_texture(
                update.texture_id,
                update.width,
                update.height,
                update.format,
                &update.pixels,
                update.content_hash,
            )?;
        }
        self.width = frame.width;
        self.height = frame.height;
        self.textures.retain(|texture_id, _| {
            *texture_id == u32::MAX
                || frame
                    .draws
                    .iter()
                    .any(|draw| draw.texture_id == *texture_id)
        });
        Ok(frame)
    }

    pub fn render(&mut self, frame: LegacyRenderFrameV1) -> Result<Vec<u8>, String> {
        let frame = self.prepare(frame)?;
        self.render_prepared(&frame)
    }

    pub fn render_prepared(&mut self, frame: &LegacyRenderFrameV1) -> Result<Vec<u8>, String> {
        if !frame.texture_updates.is_empty()
            || frame.width != self.width
            || frame.height != self.height
        {
            return Err("ASTRA_EMU_HEADLESS_FRAME_NOT_PREPARED".into());
        }
        self.rgba8 = vec![0; checked_len(frame.width, frame.height, 4)?];
        for alpha in self.rgba8[3..].iter_mut().step_by(4) {
            *alpha = 255;
        }
        for draw in &frame.draws {
            self.draw(draw)?;
        }
        Ok(std::mem::take(&mut self.rgba8))
    }

    pub fn dimensions(&self) -> Option<(u32, u32)> {
        (self.width != 0 && self.height != 0).then_some((self.width, self.height))
    }

    fn insert_texture(
        &mut self,
        texture_id: u32,
        width: u32,
        height: u32,
        format: LegacyTextureFormat,
        pixels: &[u8],
        content_hash: Hash256,
    ) -> Result<(), String> {
        Self::insert_texture_into(
            &mut self.textures,
            texture_id,
            width,
            height,
            format,
            pixels,
            content_hash,
        )
    }

    fn insert_texture_into(
        textures: &mut BTreeMap<u32, Arc<Texture>>,
        texture_id: u32,
        width: u32,
        height: u32,
        format: LegacyTextureFormat,
        pixels: &[u8],
        content_hash: Hash256,
    ) -> Result<(), String> {
        if Hash256::from_sha256(pixels) != content_hash {
            return Err("ASTRA_EMU_HEADLESS_TEXTURE_HASH".into());
        }
        let rgba8 = rgba8_pixels(width, height, format, pixels)?;
        textures.insert(
            texture_id,
            Arc::new(Texture {
                width,
                height,
                rgba8,
            }),
        );
        Ok(())
    }

    fn update_texture_into(
        textures: &mut BTreeMap<u32, Arc<Texture>>,
        texture_id: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        format: LegacyTextureFormat,
        pixels: &[u8],
        content_hash: Hash256,
    ) -> Result<(), String> {
        if Hash256::from_sha256(pixels) != content_hash {
            return Err("ASTRA_EMU_HEADLESS_TEXTURE_HASH".into());
        }
        let previous = textures
            .get(&texture_id)
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_TEXTURE_MISSING".to_owned())?;
        let right = x
            .checked_add(width)
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_TEXTURE_REGION".to_owned())?;
        let bottom = y
            .checked_add(height)
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_TEXTURE_REGION".to_owned())?;
        if right > previous.width || bottom > previous.height {
            return Err("ASTRA_EMU_HEADLESS_TEXTURE_REGION".into());
        }
        let update = rgba8_pixels(width, height, format, pixels)?;
        let mut next = (**previous).clone();
        let row_bytes = usize::try_from(width)
            .ok()
            .and_then(|value| value.checked_mul(4))
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_TEXTURE_REGION".to_owned())?;
        for row in 0..height {
            let destination = usize::try_from(y + row)
                .ok()
                .and_then(|row| {
                    usize::try_from(previous.width)
                        .ok()
                        .and_then(|stride| row.checked_mul(stride))
                })
                .and_then(|offset| usize::try_from(x).ok().and_then(|x| offset.checked_add(x)))
                .and_then(|offset| offset.checked_mul(4))
                .ok_or_else(|| "ASTRA_EMU_HEADLESS_TEXTURE_REGION".to_owned())?;
            let source = usize::try_from(row)
                .ok()
                .and_then(|row| row.checked_mul(row_bytes))
                .ok_or_else(|| "ASTRA_EMU_HEADLESS_TEXTURE_REGION".to_owned())?;
            next.rgba8[destination..destination + row_bytes]
                .copy_from_slice(&update[source..source + row_bytes]);
        }
        textures.insert(texture_id, Arc::new(next));
        Ok(())
    }

    fn draw(&mut self, draw: &LegacyDrawV1) -> Result<(), String> {
        let texture = if draw.texture_id == u32::MAX {
            Arc::new(Texture {
                width: 1,
                height: 1,
                rgba8: vec![255, 255, 255, 255],
            })
        } else {
            self.textures
                .get(&draw.texture_id)
                .cloned()
                .ok_or_else(|| "ASTRA_EMU_HEADLESS_TEXTURE_MISSING".to_owned())?
        };
        let (clip_x0, clip_y0, clip_x1, clip_y1) = if let Some(scissor) = draw.scissor {
            if scissor.x < 0 || scissor.y < 0 || scissor.width <= 0 || scissor.height <= 0 {
                return Err("ASTRA_EMU_HEADLESS_SCISSOR_INVALID".into());
            }
            let x1 = scissor
                .x
                .checked_add(scissor.width)
                .ok_or_else(|| "ASTRA_EMU_HEADLESS_SCISSOR_BOUNDS".to_owned())?;
            let y1 = scissor
                .y
                .checked_add(scissor.height)
                .ok_or_else(|| "ASTRA_EMU_HEADLESS_SCISSOR_BOUNDS".to_owned())?;
            if x1 > self.width as i32 || y1 > self.height as i32 {
                return Err("ASTRA_EMU_HEADLESS_SCISSOR_BOUNDS".into());
            }
            (scissor.x, scissor.y, x1, y1)
        } else {
            (0, 0, self.width as i32, self.height as i32)
        };
        for triangle in [[0, 1, 2], [2, 1, 3]] {
            self.draw_triangle(
                texture.as_ref(),
                draw.blend,
                [
                    draw.vertices[triangle[0]],
                    draw.vertices[triangle[1]],
                    draw.vertices[triangle[2]],
                ],
                (clip_x0, clip_y0, clip_x1, clip_y1),
            )?;
        }
        Ok(())
    }

    fn draw_triangle(
        &mut self,
        texture: &Texture,
        blend: LegacyBlendMode,
        vertices: [LegacyVertexV1; 3],
        clip: (i32, i32, i32, i32),
    ) -> Result<(), String> {
        if vertices
            .iter()
            .flat_map(|vertex| {
                vertex
                    .position
                    .iter()
                    .chain(vertex.tex_coord.iter())
                    .chain(vertex.color.iter())
            })
            .any(|value| !value.is_finite())
        {
            return Err("ASTRA_EMU_HEADLESS_VERTEX_INVALID".into());
        }
        let area = edge(
            vertices[0].position,
            vertices[1].position,
            vertices[2].position,
        );
        if area.abs() <= f32::EPSILON {
            return Ok(());
        }
        let inv_area = 1.0 / area;
        let min_x = vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::INFINITY, f32::min)
            .floor() as i32;
        let max_x = vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil() as i32;
        let min_y = vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::INFINITY, f32::min)
            .floor() as i32;
        let max_y = vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil() as i32;
        let x0 = min_x.max(clip.0);
        let y0 = min_y.max(clip.1);
        let x1 = max_x.min(clip.2);
        let y1 = max_y.min(clip.3);
        if x0 >= x1 || y0 >= y1 {
            return Ok(());
        }
        let x0 = usize::try_from(x0).map_err(|_| "ASTRA_EMU_HEADLESS_FRAME_BOUNDS".to_owned())?;
        let x1 = usize::try_from(x1).map_err(|_| "ASTRA_EMU_HEADLESS_FRAME_BOUNDS".to_owned())?;
        let y0 = usize::try_from(y0).map_err(|_| "ASTRA_EMU_HEADLESS_FRAME_BOUNDS".to_owned())?;
        let y1 = usize::try_from(y1).map_err(|_| "ASTRA_EMU_HEADLESS_FRAME_BOUNDS".to_owned())?;
        let row_bytes = usize::try_from(self.width)
            .ok()
            .and_then(|width| width.checked_mul(4))
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_FRAME_BOUNDS".to_owned())?;
        self.rgba8
            .par_chunks_mut(row_bytes)
            .enumerate()
            .skip(y0)
            .take(y1 - y0)
            .for_each(|(y, row)| {
                for x in x0..x1 {
                    let point = [x as f32 + 0.5, y as f32 + 0.5];
                    let weights = [
                        edge(vertices[1].position, vertices[2].position, point) * inv_area,
                        edge(vertices[2].position, vertices[0].position, point) * inv_area,
                        edge(vertices[0].position, vertices[1].position, point) * inv_area,
                    ];
                    if weights.iter().any(|weight| *weight < 0.0) {
                        continue;
                    }
                    let uv = interpolate2(&vertices, weights, |vertex| vertex.tex_coord);
                    let color = interpolate4(&vertices, weights, |vertex| vertex.color);
                    let mut source = sample_linear(texture, uv);
                    for channel in 0..4 {
                        source[channel] *= color[channel];
                    }
                    let index = x * 4;
                    let destination = [
                        f32::from(row[index]) / 255.0,
                        f32::from(row[index + 1]) / 255.0,
                        f32::from(row[index + 2]) / 255.0,
                        f32::from(row[index + 3]) / 255.0,
                    ];
                    let output = blend_pixel(source, destination, blend);
                    row[index..index + 4].copy_from_slice(&[
                        encode_unorm(output[0]),
                        encode_unorm(output[1]),
                        encode_unorm(output[2]),
                        encode_unorm(output[3]),
                    ]);
                }
            });
        Ok(())
    }
}

fn rgba8_pixels(
    width: u32,
    height: u32,
    format: LegacyTextureFormat,
    pixels: &[u8],
) -> Result<Vec<u8>, String> {
    let source_channels = match format {
        LegacyTextureFormat::Rgba8 => 4,
        LegacyTextureFormat::LumaAlpha8 => 2,
    };
    if pixels.len() != checked_len(width, height, source_channels)? {
        return Err("ASTRA_EMU_HEADLESS_TEXTURE_LENGTH".into());
    }
    Ok(match format {
        LegacyTextureFormat::Rgba8 => pixels.to_vec(),
        LegacyTextureFormat::LumaAlpha8 => pixels
            .chunks_exact(2)
            .flat_map(|pair| [pair[0], pair[0], pair[0], pair[1]])
            .collect(),
    })
}

fn checked_len(width: u32, height: u32, channels: usize) -> Result<usize, String> {
    usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(channels))
        .ok_or_else(|| "ASTRA_EMU_HEADLESS_FRAME_BOUNDS".to_owned())
}

fn edge(a: [f32; 2], b: [f32; 2], p: [f32; 2]) -> f32 {
    (p[0] - a[0]) * (b[1] - a[1]) - (p[1] - a[1]) * (b[0] - a[0])
}

fn interpolate2(
    vertices: &[LegacyVertexV1; 3],
    weights: [f32; 3],
    field: impl Fn(&LegacyVertexV1) -> [f32; 2],
) -> [f32; 2] {
    let values = [
        field(&vertices[0]),
        field(&vertices[1]),
        field(&vertices[2]),
    ];
    [0, 1].map(|channel| {
        weights[0] * values[0][channel]
            + weights[1] * values[1][channel]
            + weights[2] * values[2][channel]
    })
}

fn interpolate4(
    vertices: &[LegacyVertexV1; 3],
    weights: [f32; 3],
    field: impl Fn(&LegacyVertexV1) -> [f32; 4],
) -> [f32; 4] {
    let values = [
        field(&vertices[0]),
        field(&vertices[1]),
        field(&vertices[2]),
    ];
    [0, 1, 2, 3].map(|channel| {
        weights[0] * values[0][channel]
            + weights[1] * values[1][channel]
            + weights[2] * values[2][channel]
    })
}

fn sample_linear(texture: &Texture, uv: [f32; 2]) -> [f32; 4] {
    let x = uv[0].clamp(0.0, 1.0) * texture.width.saturating_sub(1) as f32;
    let y = uv[1].clamp(0.0, 1.0) * texture.height.saturating_sub(1) as f32;
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(texture.width - 1);
    let y1 = (y0 + 1).min(texture.height - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let values = [
        texel(texture, x0, y0),
        texel(texture, x1, y0),
        texel(texture, x0, y1),
        texel(texture, x1, y1),
    ];
    [0, 1, 2, 3].map(|channel| {
        let top = values[0][channel] + (values[1][channel] - values[0][channel]) * tx;
        let bottom = values[2][channel] + (values[3][channel] - values[2][channel]) * tx;
        top + (bottom - top) * ty
    })
}

fn texel(texture: &Texture, x: u32, y: u32) -> [f32; 4] {
    let offset = ((y as usize * texture.width as usize) + x as usize) * 4;
    [
        f32::from(texture.rgba8[offset]) / 255.0,
        f32::from(texture.rgba8[offset + 1]) / 255.0,
        f32::from(texture.rgba8[offset + 2]) / 255.0,
        f32::from(texture.rgba8[offset + 3]) / 255.0,
    ]
}

fn blend_pixel(source: [f32; 4], destination: [f32; 4], mode: LegacyBlendMode) -> [f32; 4] {
    let alpha = source[3].clamp(0.0, 1.0);
    let color = match mode {
        LegacyBlendMode::Alpha => {
            [0, 1, 2].map(|channel| source[channel] * alpha + destination[channel] * (1.0 - alpha))
        }
        LegacyBlendMode::Add => {
            [0, 1, 2].map(|channel| source[channel] * alpha + destination[channel])
        }
        LegacyBlendMode::Multiply => {
            [0, 1, 2].map(|channel| source[channel] * destination[channel])
        }
    };
    [
        color[0].clamp(0.0, 1.0),
        color[1].clamp(0.0, 1.0),
        color[2].clamp(0.0, 1.0),
        (alpha + destination[3] * (1.0 - alpha)).clamp(0.0, 1.0),
    ]
}

fn encode_unorm(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use astra_emu_family_api::{
        LegacyScenePacketV1, LegacySceneResourceOperationV1, LegacySceneTextureCreateV1,
        LegacySceneTextureUpdateV1, LegacyTextureUpdateV1, LegacyVertexV1,
    };

    use super::*;

    #[test]
    fn renders_textured_quad_and_preserves_texture_across_frames() {
        let pixels = vec![255, 0, 0, 255];
        let draw = LegacyDrawV1 {
            texture_id: 1,
            vertices: [
                vertex(0.0, 0.0, 0.0, 0.0),
                vertex(2.0, 0.0, 1.0, 0.0),
                vertex(0.0, 2.0, 0.0, 1.0),
                vertex(2.0, 2.0, 1.0, 1.0),
            ],
            blend: LegacyBlendMode::Alpha,
            scissor: None,
        };
        let mut rasterizer = CpuStageRasterizer::default();
        let first = rasterizer
            .render(LegacyRenderFrameV1 {
                width: 2,
                height: 2,
                texture_updates: vec![LegacyTextureUpdateV1 {
                    texture_id: 1,
                    width: 1,
                    height: 1,
                    format: LegacyTextureFormat::Rgba8,
                    content_hash: Hash256::from_sha256(&pixels),
                    pixels,
                }],
                draws: vec![draw.clone()],
            })
            .unwrap();
        let second = rasterizer
            .render(LegacyRenderFrameV1 {
                width: 2,
                height: 2,
                texture_updates: Vec::new(),
                draws: vec![draw],
            })
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(&first[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn prepared_unsampled_frames_preserve_texture_state_for_later_raster() {
        let pixels = vec![12, 34, 56, 255];
        let draw = LegacyDrawV1 {
            texture_id: 9,
            vertices: [
                vertex(0.0, 0.0, 0.0, 0.0),
                vertex(1.0, 0.0, 1.0, 0.0),
                vertex(0.0, 1.0, 0.0, 1.0),
                vertex(1.0, 1.0, 1.0, 1.0),
            ],
            blend: LegacyBlendMode::Alpha,
            scissor: None,
        };
        let mut rasterizer = CpuStageRasterizer::default();
        let _skipped = rasterizer
            .prepare(LegacyRenderFrameV1 {
                width: 1,
                height: 1,
                texture_updates: vec![LegacyTextureUpdateV1 {
                    texture_id: 9,
                    width: 1,
                    height: 1,
                    format: LegacyTextureFormat::Rgba8,
                    content_hash: Hash256::from_sha256(&pixels),
                    pixels,
                }],
                draws: vec![draw.clone()],
            })
            .unwrap();
        let sampled = rasterizer
            .prepare(LegacyRenderFrameV1 {
                width: 1,
                height: 1,
                texture_updates: Vec::new(),
                draws: vec![draw],
            })
            .unwrap();

        assert_eq!(
            rasterizer.render_prepared(&sampled).unwrap(),
            vec![12, 34, 56, 255]
        );
    }

    #[test]
    fn evicts_textures_not_referenced_by_the_committed_frame() {
        let mut rasterizer = CpuStageRasterizer::default();
        rasterizer.textures.insert(
            7,
            Arc::new(Texture {
                width: 1,
                height: 1,
                rgba8: vec![0, 0, 0, 255],
            }),
        );
        rasterizer
            .render(LegacyRenderFrameV1 {
                width: 1,
                height: 1,
                texture_updates: Vec::new(),
                draws: Vec::new(),
            })
            .unwrap();
        assert!(!rasterizer.textures.contains_key(&7));
    }

    #[test]
    fn blends_rfvp_texture_channels_as_unorm_values() {
        let texture = Texture {
            width: 1,
            height: 1,
            rgba8: vec![128, 64, 32, 255],
        };
        assert_eq!(
            texel(&texture, 0, 0),
            [128.0 / 255.0, 64.0 / 255.0, 32.0 / 255.0, 1.0]
        );
        let result = blend_pixel(
            [0.0, 0.0, 0.0, 0.5],
            [1.0, 1.0, 1.0, 1.0],
            LegacyBlendMode::Alpha,
        );
        assert_eq!(result, [0.5, 0.5, 0.5, 1.0]);
        assert_eq!(encode_unorm(result[0]), 128);
    }

    #[test]
    fn semantic_scene_commit_retains_and_partially_updates_texture() {
        let draw = LegacyDrawV1 {
            texture_id: 42,
            vertices: [
                vertex(0.0, 0.0, 0.0, 0.0),
                vertex(2.0, 0.0, 1.0, 0.0),
                vertex(0.0, 1.0, 0.0, 1.0),
                vertex(2.0, 1.0, 1.0, 1.0),
            ],
            blend: LegacyBlendMode::Alpha,
            scissor: None,
        };
        let first_pixels = vec![255, 0, 0, 255, 0, 255, 0, 255];
        let first_packet = LegacyScenePacketV1 {
            width: 2,
            height: 1,
            resources: vec![LegacySceneResourceOperationV1::CreateTexture(
                LegacySceneTextureCreateV1 {
                    texture_id: 42,
                    width: 2,
                    height: 1,
                    format: LegacyTextureFormat::Rgba8,
                    content_hash: Hash256::from_sha256(&first_pixels),
                    pixels: first_pixels,
                },
            )],
            draws: vec![draw.clone()],
        };
        let mut rasterizer = CpuStageRasterizer::default();
        let first = rasterizer
            .prepare_scene_commit(
                LegacySceneResourceStateV1::default()
                    .prepare(first_packet)
                    .unwrap(),
            )
            .unwrap();
        assert!(first.texture_updates.is_empty());
        assert_eq!(
            rasterizer.textures.get(&42).unwrap().rgba8,
            vec![255, 0, 0, 255, 0, 255, 0, 255]
        );

        let patch = vec![0, 0, 255, 255];
        let update_packet = LegacyScenePacketV1 {
            width: 2,
            height: 1,
            resources: vec![LegacySceneResourceOperationV1::UpdateTexture(
                LegacySceneTextureUpdateV1 {
                    texture_id: 42,
                    x: 1,
                    y: 0,
                    width: 1,
                    height: 1,
                    format: LegacyTextureFormat::Rgba8,
                    content_hash: Hash256::from_sha256(&patch),
                    pixels: patch,
                },
            )],
            draws: vec![draw],
        };
        let second_commit = rasterizer.scene_resources.prepare(update_packet).unwrap();
        let second = rasterizer.prepare_scene_commit(second_commit).unwrap();
        assert!(second.texture_updates.is_empty());
        assert_eq!(
            rasterizer.textures.get(&42).unwrap().rgba8,
            vec![255, 0, 0, 255, 0, 0, 255, 255]
        );
    }

    fn vertex(x: f32, y: f32, u: f32, v: f32) -> LegacyVertexV1 {
        LegacyVertexV1 {
            position: [x, y],
            tex_coord: [u, v],
            color: [1.0; 4],
        }
    }
}
