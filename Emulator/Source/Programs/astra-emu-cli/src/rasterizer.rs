use std::{collections::BTreeMap, sync::Arc};

use astra_emu_family_api::{
    LegacyBlendMode, LegacyDrawV1, LegacyRenderFrameV1, LegacySceneResourceStateV1,
    LegacyTextureFilter, LegacyTextureFormat, LegacyVertexV1,
};
use astra_emu_manager_core::legacy_texture_format as live_texture_format;
use astra_media_core::OwnedPixelBuffer;
use astra_plugin_abi::{
    RuntimeLiveBlendMode, RuntimeLiveSceneCompositing, RuntimeLiveSceneResourceOperation,
    RuntimeLiveSceneTransaction, RuntimeLiveTextureFilter,
};
use rayon::prelude::*;

#[derive(Clone)]
struct Texture {
    width: u32,
    height: u32,
    rgba8: OwnedPixelBuffer,
}

#[derive(Default)]
pub struct CpuStageRasterizer {
    textures: BTreeMap<u32, Arc<Texture>>,
    scene_resources: LegacySceneResourceStateV1,
    width: u32,
    height: u32,
    rgba8: Vec<u8>,
    compositing: RuntimeLiveSceneCompositing,
}

pub enum PreparedRenderFrame {
    Legacy(LegacyRenderFrameV1),
    Live {
        width: u32,
        height: u32,
        draws: Vec<astra_plugin_abi::RuntimeLiveDraw>,
    },
}

impl PreparedRenderFrame {
    pub fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::Legacy(frame) => (frame.width, frame.height),
            Self::Live { width, height, .. } => (*width, *height),
        }
    }
}

impl CpuStageRasterizer {
    /// Applies a Family ABI v7 scene transaction without constructing a
    /// serialized packet or hashing its pixels.  The retained CPU texture is
    /// the destination allocation; only the explicitly required LumaAlpha8
    /// conversion creates a new RGBA buffer.
    pub fn prepare_scene_live(
        &mut self,
        transaction: RuntimeLiveSceneTransaction,
    ) -> Result<PreparedRenderFrame, String> {
        transaction.validate().map_err(|error| error.to_string())?;
        if transaction.reset_resources {
            self.textures.clear();
            self.scene_resources = LegacySceneResourceStateV1::default();
        }
        for operation in transaction.resources {
            match operation {
                RuntimeLiveSceneResourceOperation::CreateTexture {
                    texture_id,
                    width,
                    height,
                    format,
                    pixels,
                    ..
                } => {
                    if self.textures.contains_key(&texture_id) {
                        return Err("ASTRA_EMU_HEADLESS_LIVE_TEXTURE_EXISTS".into());
                    }
                    let format = live_texture_format(format);
                    let rgba8 = rgba8_pixels_owned(width, height, format, pixels)?;
                    self.textures.insert(
                        texture_id,
                        Arc::new(Texture {
                            width,
                            height,
                            rgba8,
                        }),
                    );
                    self.scene_resources.textures.insert(
                        texture_id,
                        astra_emu_family_api::LegacySceneTextureDescriptorV1 {
                            width,
                            height,
                            format,
                        },
                    );
                }
                RuntimeLiveSceneResourceOperation::UpdateTexture {
                    texture_id,
                    x,
                    y,
                    width,
                    height,
                    format,
                    pixels,
                    ..
                } => {
                    let format = live_texture_format(format);
                    let texture = self
                        .textures
                        .get_mut(&texture_id)
                        .ok_or_else(|| "ASTRA_EMU_HEADLESS_LIVE_TEXTURE_MISSING".to_owned())?;
                    let retained_format = self
                        .scene_resources
                        .textures
                        .get(&texture_id)
                        .map(|descriptor| descriptor.format)
                        .ok_or_else(|| "ASTRA_EMU_HEADLESS_LIVE_TEXTURE_MISSING".to_owned())?;
                    if retained_format != format
                        || x.checked_add(width)
                            .is_none_or(|right| right > texture.width)
                        || y.checked_add(height)
                            .is_none_or(|bottom| bottom > texture.height)
                    {
                        return Err("ASTRA_EMU_HEADLESS_LIVE_TEXTURE_REGION".into());
                    }
                    let rgba8 = rgba8_pixels_owned(width, height, format, pixels)?;
                    let texture = Arc::make_mut(texture);
                    let retained = texture.rgba8.make_mut_for_update();
                    for row in 0..height as usize {
                        let source = row * width as usize * 4;
                        let target = ((y as usize + row) * texture.width as usize + x as usize) * 4;
                        retained[target..target + width as usize * 4]
                            .copy_from_slice(&rgba8[source..source + width as usize * 4]);
                    }
                }
                RuntimeLiveSceneResourceOperation::DestroyTexture { texture_id, .. } => {
                    if self.textures.remove(&texture_id).is_none() {
                        return Err("ASTRA_EMU_HEADLESS_LIVE_TEXTURE_MISSING".into());
                    }
                    self.scene_resources.textures.remove(&texture_id);
                }
            }
        }
        self.width = transaction.width;
        self.height = transaction.height;
        self.compositing = transaction.compositing;
        Ok(PreparedRenderFrame::Live {
            width: transaction.width,
            height: transaction.height,
            draws: transaction.draws,
        })
    }

    pub fn prepare(
        &mut self,
        mut frame: LegacyRenderFrameV1,
    ) -> Result<PreparedRenderFrame, String> {
        frame.validate().map_err(|error| error.to_string())?;
        for update in std::mem::take(&mut frame.texture_updates) {
            self.insert_texture(
                update.texture_id,
                update.width,
                update.height,
                update.format,
                &update.pixels,
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
        Ok(PreparedRenderFrame::Legacy(frame))
    }

    pub fn render(&mut self, frame: LegacyRenderFrameV1) -> Result<Vec<u8>, String> {
        let frame = self.prepare(frame)?;
        self.render_prepared(&frame)
    }

    pub fn render_prepared(&mut self, frame: &PreparedRenderFrame) -> Result<Vec<u8>, String> {
        let (width, height) = match frame {
            PreparedRenderFrame::Legacy(frame) => {
                if !frame.texture_updates.is_empty() {
                    return Err("ASTRA_EMU_HEADLESS_FRAME_NOT_PREPARED".into());
                }
                (frame.width, frame.height)
            }
            PreparedRenderFrame::Live { width, height, .. } => (*width, *height),
        };
        if width != self.width || height != self.height {
            return Err("ASTRA_EMU_HEADLESS_FRAME_NOT_PREPARED".into());
        }
        self.rgba8 = vec![0; checked_len(width, height, 4)?];
        for alpha in self.rgba8[3..].iter_mut().step_by(4) {
            *alpha = 255;
        }
        match frame {
            PreparedRenderFrame::Legacy(frame) => {
                for draw in &frame.draws {
                    self.draw(draw)?;
                }
            }
            PreparedRenderFrame::Live { draws, .. } => {
                for draw in draws {
                    self.draw_runtime(draw)?;
                }
            }
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
    ) -> Result<(), String> {
        Self::insert_texture_into(
            &mut self.textures,
            texture_id,
            width,
            height,
            format,
            pixels,
        )
    }

    fn insert_texture_into(
        textures: &mut BTreeMap<u32, Arc<Texture>>,
        texture_id: u32,
        width: u32,
        height: u32,
        format: LegacyTextureFormat,
        pixels: &[u8],
    ) -> Result<(), String> {
        let rgba8 = rgba8_pixels(width, height, format, pixels)?;
        textures.insert(
            texture_id,
            Arc::new(Texture {
                width,
                height,
                rgba8: rgba8.into(),
            }),
        );
        Ok(())
    }

    fn draw(&mut self, draw: &LegacyDrawV1) -> Result<(), String> {
        let texture = if draw.texture_id == u32::MAX {
            Arc::new(Texture {
                width: 1,
                height: 1,
                rgba8: vec![255, 255, 255, 255].into(),
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
                draw.texture_filter,
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

    fn draw_runtime(&mut self, draw: &astra_plugin_abi::RuntimeLiveDraw) -> Result<(), String> {
        let draw = live_draw(draw)?;
        self.draw(&draw)
    }

    fn draw_triangle(
        &mut self,
        texture: &Texture,
        blend: LegacyBlendMode,
        texture_filter: LegacyTextureFilter,
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
        let compositing = self.compositing;
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
                    let mut source = match (compositing, texture_filter) {
                        (RuntimeLiveSceneCompositing::LinearSrgb, LegacyTextureFilter::Nearest) => {
                            sample_nearest(texture, uv)
                        }
                        (RuntimeLiveSceneCompositing::LinearSrgb, LegacyTextureFilter::Linear) => {
                            sample_linear(texture, uv)
                        }
                        (
                            RuntimeLiveSceneCompositing::EncodedSrgb,
                            LegacyTextureFilter::Nearest,
                        ) => sample_nearest_encoded(texture, uv),
                        (RuntimeLiveSceneCompositing::EncodedSrgb, LegacyTextureFilter::Linear) => {
                            sample_linear_encoded(texture, uv)
                        }
                    };
                    for channel in 0..4 {
                        source[channel] *= color[channel];
                    }
                    let index = x * 4;
                    let destination = match compositing {
                        RuntimeLiveSceneCompositing::LinearSrgb => [
                            srgb_byte_to_linear(row[index]),
                            srgb_byte_to_linear(row[index + 1]),
                            srgb_byte_to_linear(row[index + 2]),
                            f32::from(row[index + 3]) / 255.0,
                        ],
                        RuntimeLiveSceneCompositing::EncodedSrgb => [
                            f32::from(row[index]) / 255.0,
                            f32::from(row[index + 1]) / 255.0,
                            f32::from(row[index + 2]) / 255.0,
                            f32::from(row[index + 3]) / 255.0,
                        ],
                    };
                    let output = blend_pixel(source, destination, blend);
                    let rgb = match compositing {
                        RuntimeLiveSceneCompositing::LinearSrgb => [
                            linear_to_srgb_byte(output[0]),
                            linear_to_srgb_byte(output[1]),
                            linear_to_srgb_byte(output[2]),
                        ],
                        RuntimeLiveSceneCompositing::EncodedSrgb => [
                            encode_unorm(output[0]),
                            encode_unorm(output[1]),
                            encode_unorm(output[2]),
                        ],
                    };
                    row[index..index + 4].copy_from_slice(&[
                        rgb[0],
                        rgb[1],
                        rgb[2],
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
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|pair| [pair[0], pair[0], pair[0], pair[1]])
            .collect(),
    })
}

fn rgba8_pixels_owned(
    width: u32,
    height: u32,
    format: LegacyTextureFormat,
    pixels: astra_byte_source::OwnedByteBuffer,
) -> Result<OwnedPixelBuffer, String> {
    let channels = format.bytes_per_pixel();
    if pixels.len() != checked_len(width, height, channels)? {
        return Err("ASTRA_EMU_HEADLESS_TEXTURE_LENGTH".into());
    }
    Ok(match format {
        LegacyTextureFormat::Rgba8 => OwnedPixelBuffer::from_owned(pixels),
        LegacyTextureFormat::LumaAlpha8 => OwnedPixelBuffer::from_vec(
            pixels
                .as_slice()
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|pair| [pair[0], pair[0], pair[0], pair[1]])
                .collect(),
        ),
    })
}

fn live_draw(draw: &astra_plugin_abi::RuntimeLiveDraw) -> Result<LegacyDrawV1, String> {
    let vertices = draw
        .vertices
        .map(|vertex| astra_emu_family_api::LegacyVertexV1 {
            position: [vertex.x, vertex.y],
            tex_coord: [vertex.u, vertex.v],
            color: vertex.color.map(|channel| f32::from(channel) / 255.0),
        });
    let scissor = draw
        .scissor
        .map(
            |scissor| -> Result<astra_emu_family_api::LegacyScissorV1, String> {
                Ok(astra_emu_family_api::LegacyScissorV1 {
                    x: i32::try_from(scissor.x).map_err(|_| "ASTRA_EMU_LIVE_SCISSOR_BOUNDS")?,
                    y: i32::try_from(scissor.y).map_err(|_| "ASTRA_EMU_LIVE_SCISSOR_BOUNDS")?,
                    width: i32::try_from(scissor.width)
                        .map_err(|_| "ASTRA_EMU_LIVE_SCISSOR_BOUNDS")?,
                    height: i32::try_from(scissor.height)
                        .map_err(|_| "ASTRA_EMU_LIVE_SCISSOR_BOUNDS")?,
                })
            },
        )
        .transpose()?;
    Ok(LegacyDrawV1 {
        texture_id: draw.texture_id,
        vertices,
        blend: match draw.blend {
            RuntimeLiveBlendMode::Alpha => LegacyBlendMode::Alpha,
            RuntimeLiveBlendMode::Additive => LegacyBlendMode::Add,
            RuntimeLiveBlendMode::Opaque => LegacyBlendMode::Opaque,
            RuntimeLiveBlendMode::Multiply => LegacyBlendMode::Multiply,
            RuntimeLiveBlendMode::Screen => LegacyBlendMode::Screen,
        },
        texture_filter: match draw.texture_filter {
            RuntimeLiveTextureFilter::Nearest => LegacyTextureFilter::Nearest,
            RuntimeLiveTextureFilter::Linear => LegacyTextureFilter::Linear,
        },
        scissor,
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

fn sample_nearest(texture: &Texture, uv: [f32; 2]) -> [f32; 4] {
    let x = (uv[0].clamp(0.0, 1.0) * texture.width.saturating_sub(1) as f32).round() as u32;
    let y = (uv[1].clamp(0.0, 1.0) * texture.height.saturating_sub(1) as f32).round() as u32;
    texel(texture, x, y)
}

fn sample_linear_encoded(texture: &Texture, uv: [f32; 2]) -> [f32; 4] {
    let x = uv[0].clamp(0.0, 1.0) * texture.width.saturating_sub(1) as f32;
    let y = uv[1].clamp(0.0, 1.0) * texture.height.saturating_sub(1) as f32;
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(texture.width - 1);
    let y1 = (y0 + 1).min(texture.height - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let values = [
        texel_encoded(texture, x0, y0),
        texel_encoded(texture, x1, y0),
        texel_encoded(texture, x0, y1),
        texel_encoded(texture, x1, y1),
    ];
    [0, 1, 2, 3].map(|channel| {
        let top = values[0][channel] + (values[1][channel] - values[0][channel]) * tx;
        let bottom = values[2][channel] + (values[3][channel] - values[2][channel]) * tx;
        top + (bottom - top) * ty
    })
}

fn sample_nearest_encoded(texture: &Texture, uv: [f32; 2]) -> [f32; 4] {
    let x = (uv[0].clamp(0.0, 1.0) * texture.width.saturating_sub(1) as f32).round() as u32;
    let y = (uv[1].clamp(0.0, 1.0) * texture.height.saturating_sub(1) as f32).round() as u32;
    texel_encoded(texture, x, y)
}

fn texel_encoded(texture: &Texture, x: u32, y: u32) -> [f32; 4] {
    let offset = ((y as usize * texture.width as usize) + x as usize) * 4;
    [
        f32::from(texture.rgba8[offset]) / 255.0,
        f32::from(texture.rgba8[offset + 1]) / 255.0,
        f32::from(texture.rgba8[offset + 2]) / 255.0,
        f32::from(texture.rgba8[offset + 3]) / 255.0,
    ]
}

fn texel(texture: &Texture, x: u32, y: u32) -> [f32; 4] {
    let offset = ((y as usize * texture.width as usize) + x as usize) * 4;
    [
        srgb_byte_to_linear(texture.rgba8[offset]),
        srgb_byte_to_linear(texture.rgba8[offset + 1]),
        srgb_byte_to_linear(texture.rgba8[offset + 2]),
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
        LegacyBlendMode::Opaque => [source[0], source[1], source[2]],
        LegacyBlendMode::Multiply => {
            [0, 1, 2].map(|channel| source[channel] * destination[channel])
        }
        LegacyBlendMode::Screen => [
            1.0 - (1.0 - source[0]) * (1.0 - destination[0]),
            1.0 - (1.0 - source[1]) * (1.0 - destination[1]),
            1.0 - (1.0 - source[2]) * (1.0 - destination[2]),
        ],
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

fn srgb_byte_to_linear(value: u8) -> f32 {
    let encoded = f32::from(value) / 255.0;
    if encoded <= 0.04045 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb_byte(value: f32) -> u8 {
    let linear = value.clamp(0.0, 1.0);
    let encoded = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use astra_emu_family_api::{LegacyTextureUpdateV1, LegacyVertexV1};
    use astra_plugin_abi::{RuntimeLiveSceneResourceOperation, RuntimeLiveTextureFormat};

    use super::*;

    #[test]
    fn live_rgba8_capture_allocation_is_retained_without_rebuild() {
        let pixels = vec![12, 34, 56, 255];
        let capture_ptr = pixels.as_ptr();
        let transaction = RuntimeLiveSceneTransaction {
            sequence: 1,
            width: 1,
            height: 1,
            compositing: RuntimeLiveSceneCompositing::EncodedSrgb,
            resources: vec![RuntimeLiveSceneResourceOperation::CreateTexture {
                texture_id: 7,
                generation: 1,
                width: 1,
                height: 1,
                format: RuntimeLiveTextureFormat::Rgba8,
                pixels: pixels.into(),
            }],
            draws: Vec::new(),
            reset_resources: false,
        };
        let mut rasterizer = CpuStageRasterizer::default();

        let prepared = rasterizer.prepare_scene_live(transaction).unwrap();

        assert!(matches!(prepared, PreparedRenderFrame::Live { .. }));
        assert_eq!(
            rasterizer.textures.get(&7).unwrap().rgba8.allocation_ptr(),
            capture_ptr
        );
    }

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
            texture_filter: LegacyTextureFilter::Linear,
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
            texture_filter: LegacyTextureFilter::Linear,
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
                rgba8: vec![0, 0, 0, 255].into(),
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
    fn linear_compositing_decodes_srgb_texture_channels_before_blending() {
        let texture = Texture {
            width: 1,
            height: 1,
            rgba8: vec![128, 64, 32, 255].into(),
        };
        let sampled = texel(&texture, 0, 0);
        assert!((sampled[0] - srgb_byte_to_linear(128)).abs() < 1.0e-6);
        assert!((sampled[1] - srgb_byte_to_linear(64)).abs() < 1.0e-6);
        assert!((sampled[2] - srgb_byte_to_linear(32)).abs() < 1.0e-6);
        assert_eq!(sampled[3], 1.0);
        let result = blend_pixel(
            [0.0, 0.0, 0.0, 0.5],
            [1.0, 1.0, 1.0, 1.0],
            LegacyBlendMode::Alpha,
        );
        assert_eq!(result, [0.5, 0.5, 0.5, 1.0]);
        assert_eq!(encode_unorm(result[0]), 128);
    }

    #[test]
    fn encoded_srgb_compositing_matches_rfvp_byte_domain_alpha() {
        let mut rasterizer = CpuStageRasterizer {
            compositing: RuntimeLiveSceneCompositing::EncodedSrgb,
            ..Default::default()
        };
        let white = LegacyDrawV1 {
            texture_id: 1,
            vertices: [
                vertex(0.0, 0.0, 0.0, 0.0),
                vertex(2.0, 0.0, 1.0, 0.0),
                vertex(0.0, 1.0, 0.0, 1.0),
                vertex(2.0, 1.0, 1.0, 1.0),
            ],
            blend: LegacyBlendMode::Opaque,
            texture_filter: LegacyTextureFilter::Nearest,
            scissor: None,
        };
        let mut black_overlay = white.clone();
        black_overlay.texture_id = 2;
        black_overlay.blend = LegacyBlendMode::Alpha;
        for vertex in &mut black_overlay.vertices {
            vertex.color[3] = 128.0 / 255.0;
        }
        let output = rasterizer
            .render(LegacyRenderFrameV1 {
                width: 2,
                height: 1,
                texture_updates: vec![
                    LegacyTextureUpdateV1 {
                        texture_id: 1,
                        width: 1,
                        height: 1,
                        format: LegacyTextureFormat::Rgba8,
                        pixels: vec![255, 255, 255, 255],
                    },
                    LegacyTextureUpdateV1 {
                        texture_id: 2,
                        width: 1,
                        height: 1,
                        format: LegacyTextureFormat::Rgba8,
                        pixels: vec![0, 0, 0, 255],
                    },
                ],
                draws: vec![white, black_overlay],
            })
            .expect("encoded-sRGB scene renders");

        assert_eq!(output, vec![127, 127, 127, 255, 127, 127, 127, 255]);
    }

    #[test]
    fn alpha_blend_uses_straight_alpha_source_semantics() {
        let result = blend_pixel(
            [100.0 / 255.0, 50.0 / 255.0, 25.0 / 255.0, 128.0 / 255.0],
            [1.0, 1.0, 1.0, 1.0],
            LegacyBlendMode::Alpha,
        );

        assert_eq!(result.map(encode_unorm), [177, 152, 140, 255]);
    }

    #[test]
    fn texture_filter_selects_nearest_or_linear_sampling() {
        let texture = Texture {
            width: 2,
            height: 1,
            rgba8: vec![0, 0, 0, 255, 200, 100, 50, 255].into(),
        };

        assert_eq!(sample_nearest(&texture, [0.49, 0.0]), [0.0, 0.0, 0.0, 1.0]);
        let linear = sample_linear(&texture, [0.5, 0.0]);
        assert!((linear[0] - srgb_byte_to_linear(200) * 0.5).abs() < 1.0e-6);
        assert!((linear[1] - srgb_byte_to_linear(100) * 0.5).abs() < 1.0e-6);
        assert!((linear[2] - srgb_byte_to_linear(50) * 0.5).abs() < 1.0e-6);
        assert_eq!(linear[3], 1.0);
    }

    fn vertex(x: f32, y: f32, u: f32, v: f32) -> LegacyVertexV1 {
        LegacyVertexV1 {
            position: [x, y],
            tex_coord: [u, v],
            color: [1.0; 4],
        }
    }
}
