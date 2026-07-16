use std::collections::BTreeMap;

use astra_core::Hash256;
use astra_emu_family_api::{
    LegacyBlendMode, LegacyDrawV1, LegacyRenderFrameV1, LegacyTextureFormat, LegacyVertexV1,
};

#[derive(Clone)]
struct Texture {
    width: u32,
    height: u32,
    rgba8: Vec<u8>,
}

#[derive(Default)]
pub struct CpuStageRasterizer {
    textures: BTreeMap<u32, Texture>,
    width: u32,
    height: u32,
    linear_rgba: Vec<[f32; 4]>,
}

impl CpuStageRasterizer {
    pub fn render(&mut self, frame: LegacyRenderFrameV1) -> Result<Vec<u8>, String> {
        frame.validate().map_err(|error| error.to_string())?;
        for update in frame.texture_updates {
            let content_hash = Hash256::from_sha256(&update.pixels);
            if content_hash != update.content_hash {
                return Err("ASTRA_EMU_HEADLESS_TEXTURE_HASH".into());
            }
            let rgba8 = match update.format {
                LegacyTextureFormat::Rgba8 => update.pixels,
                LegacyTextureFormat::LumaAlpha8 => update
                    .pixels
                    .chunks_exact(2)
                    .flat_map(|pair| [pair[0], pair[0], pair[0], pair[1]])
                    .collect(),
            };
            let expected = checked_len(update.width, update.height, 4)?;
            if rgba8.len() != expected {
                return Err("ASTRA_EMU_HEADLESS_TEXTURE_LENGTH".into());
            }
            self.textures.insert(
                update.texture_id,
                Texture {
                    width: update.width,
                    height: update.height,
                    rgba8,
                },
            );
        }
        self.width = frame.width;
        self.height = frame.height;
        self.linear_rgba = vec![[0.0, 0.0, 0.0, 1.0]; checked_len(frame.width, frame.height, 1)?];
        for draw in &frame.draws {
            self.draw(draw)?;
        }
        Ok(self.encode_srgba8())
    }

    pub fn dimensions(&self) -> Option<(u32, u32)> {
        (self.width != 0 && self.height != 0).then_some((self.width, self.height))
    }

    fn draw(&mut self, draw: &LegacyDrawV1) -> Result<(), String> {
        let texture = self
            .textures
            .get(&draw.texture_id)
            .cloned()
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_TEXTURE_MISSING".to_owned())?;
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
                &texture,
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
        mut vertices: [LegacyVertexV1; 3],
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
        let mut area = edge(
            vertices[0].position,
            vertices[1].position,
            vertices[2].position,
        );
        if area == 0.0 {
            return Ok(());
        }
        if area < 0.0 {
            vertices.swap(1, 2);
            area = -area;
        }
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
        for y in y0..y1 {
            for x in x0..x1 {
                let point = [x as f32 + 0.5, y as f32 + 0.5];
                let w0 = edge(vertices[1].position, vertices[2].position, point);
                let w1 = edge(vertices[2].position, vertices[0].position, point);
                let w2 = edge(vertices[0].position, vertices[1].position, point);
                if !inside(w0, vertices[1].position, vertices[2].position)
                    || !inside(w1, vertices[2].position, vertices[0].position)
                    || !inside(w2, vertices[0].position, vertices[1].position)
                {
                    continue;
                }
                let barycentric = [w0 / area, w1 / area, w2 / area];
                let uv = interpolate2(&vertices, barycentric, |vertex| vertex.tex_coord);
                let color = interpolate4(&vertices, barycentric, |vertex| vertex.color);
                let mut source = sample_linear(texture, uv);
                for channel in 0..4 {
                    source[channel] *= color[channel];
                }
                let index = usize::try_from(y)
                    .ok()
                    .and_then(|y| y.checked_mul(self.width as usize))
                    .and_then(|row| usize::try_from(x).ok().and_then(|x| row.checked_add(x)))
                    .ok_or_else(|| "ASTRA_EMU_HEADLESS_FRAME_BOUNDS".to_owned())?;
                self.linear_rgba[index] = blend_pixel(source, self.linear_rgba[index], blend);
            }
        }
        Ok(())
    }

    fn encode_srgba8(&self) -> Vec<u8> {
        self.linear_rgba
            .iter()
            .flat_map(|pixel| {
                [
                    encode_srgb(pixel[0]),
                    encode_srgb(pixel[1]),
                    encode_srgb(pixel[2]),
                    encode_unorm(pixel[3]),
                ]
            })
            .collect()
    }
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
    (p[0] - a[0]).mul_add(b[1] - a[1], -((p[1] - a[1]) * (b[0] - a[0])))
}

fn inside(value: f32, a: [f32; 2], b: [f32; 2]) -> bool {
    value > 0.0 || (value == 0.0 && ((b[1] < a[1]) || (b[1] == a[1] && b[0] > a[0])))
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
    let x = (uv[0].clamp(0.0, 1.0) * texture.width as f32 - 0.5)
        .clamp(0.0, texture.width.saturating_sub(1) as f32);
    let y = (uv[1].clamp(0.0, 1.0) * texture.height as f32 - 0.5)
        .clamp(0.0, texture.height.saturating_sub(1) as f32);
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
        let top = values[0][channel] * (1.0 - tx) + values[1][channel] * tx;
        let bottom = values[2][channel] * (1.0 - tx) + values[3][channel] * tx;
        top * (1.0 - ty) + bottom * ty
    })
}

fn texel(texture: &Texture, x: u32, y: u32) -> [f32; 4] {
    let offset = ((y as usize * texture.width as usize) + x as usize) * 4;
    [
        decode_srgb(texture.rgba8[offset]),
        decode_srgb(texture.rgba8[offset + 1]),
        decode_srgb(texture.rgba8[offset + 2]),
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

fn decode_srgb(value: u8) -> f32 {
    let value = f32::from(value) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn encode_srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let encoded = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    encode_unorm(encoded)
}

fn encode_unorm(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use astra_emu_family_api::{LegacyTextureUpdateV1, LegacyVertexV1};

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

    fn vertex(x: f32, y: f32, u: f32, v: f32) -> LegacyVertexV1 {
        LegacyVertexV1 {
            position: [x, y],
            tex_coord: [u, v],
            color: [1.0; 4],
        }
    }
}
