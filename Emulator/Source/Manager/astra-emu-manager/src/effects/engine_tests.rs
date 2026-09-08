#[cfg(test)]
mod tests {
    use super::super::*;
    use std::{collections::BTreeMap, path::PathBuf};

    #[ignore = "requires the pinned local DXC DLL and a GPU adapter"]
    #[test]
    fn compiles_anime_chain_with_pinned_dxc_when_available() {
        let dxc = PathBuf::from("../../../../.tmp/dxc/dxcompiler.dll");
        assert!(
            dxc.is_file(),
            "pinned DXC DLL is required for this GPU test"
        );
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("a GPU adapter is required for this GPU test");
        if !adapter
            .features()
            .contains(FilterEngine::required_features())
        {
            panic!("required feature missing: {:?}", adapter.features());
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: FilterEngine::required_features(),
            ..Default::default()
        }))
        .unwrap();
        let mut engine = FilterEngine::new(dxc);
        engine
            .reload(
                &device,
                &FilterConfiguration {
                    preset: FilterPreset::Anime4kRestoreUpscale,
                    scale: 2.0,
                    strength: 0.35,
                    parameters: BTreeMap::new(),
                },
            )
            .unwrap();
        let input = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("effect-test-input"),
            size: wgpu::Extent3d {
                width: 17,
                height: 19,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let input_pixels = (0..19)
            .flat_map(|y| {
                (0..17).flat_map(move |x| {
                    [
                        (x * 11 + y * 3) as u8,
                        (x * 5 + y * 13) as u8,
                        (x * 17 + y * 7) as u8,
                        255,
                    ]
                })
            })
            .collect::<Vec<_>>();
        queue.write_texture(
            input.as_image_copy(),
            &input_pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(17 * 4),
                rows_per_image: Some(19),
            },
            wgpu::Extent3d {
                width: 17,
                height: 19,
                depth_or_array_layers: 1,
            },
        );
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("effect-test-output"),
            size: wgpu::Extent3d {
                width: 34,
                height: 38,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        engine
            .apply(
                &device,
                &queue,
                &input,
                &output,
                &FilterConfiguration {
                    preset: FilterPreset::Anime4kRestoreUpscale,
                    scale: 2.0,
                    strength: 0.35,
                    parameters: BTreeMap::new(),
                },
            )
            .unwrap();
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let anime_chain_pixels = readback(&device, &queue, &output, 34, 38);
        assert!(anime_chain_pixels.chunks_exact(4).all(|pixel| pixel[3] == 255));
        let anime_config = FilterConfiguration {
            preset: FilterPreset::Anime4kRestoreUpscale,
            scale: 2.0,
            strength: 0.35,
            parameters: BTreeMap::new(),
        };
        assert!(matches!(
            engine.reload(
                &device,
                &FilterConfiguration {
                    preset: FilterPreset::Anime4kRestoreUpscale,
                    scale: 1.5,
                    strength: 0.35,
                    parameters: BTreeMap::new(),
                }
            ),
            Err(FilterError::AnimeScale)
        ));
        engine
            .apply(&device, &queue, &input, &output, &anime_config)
            .unwrap();
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let retained_pixels = readback(&device, &queue, &output, 34, 38);
        assert_eq!(retained_pixels, anime_chain_pixels);
        let external_source =
            include_str!("../../../../../Assets/Effects/Anime4K/upscale_cnn_x2_s.hlsl");
        engine
            .reload_source(&device, external_source, &anime_config)
            .unwrap();
        engine
            .apply(&device, &queue, &input, &output, &anime_config)
            .unwrap();
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let external_pixels = readback(&device, &queue, &output, 34, 38);
        assert!(external_pixels.chunks_exact(4).all(|pixel| pixel[3] == 255));
        assert!(external_pixels
            .chunks_exact(4)
            .any(|pixel| pixel[..3] != [0, 0, 0]));
        for pixel in [0, 33, 34 * 37, 34 * 38 - 1] {
            assert_eq!(external_pixels[pixel * 4 + 3], 255);
        }
        let scale_output = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("effect-test-scale-output"),
            size: wgpu::Extent3d {
                width: 26,
                height: 29,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let scale_config = FilterConfiguration {
            preset: FilterPreset::Scale,
            scale: 1.5,
            strength: 0.35,
            parameters: BTreeMap::new(),
        };
        engine.reload(&device, &scale_config).unwrap();
        engine
            .apply(&device, &queue, &input, &scale_output, &scale_config)
            .unwrap();
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let scale_pixels = readback(&device, &queue, &scale_output, 26, 29);
        for (x, y) in [(0, 0), (13, 14), (25, 28)] {
            let expected = bilinear(&input_pixels, 17, 19, 26, 29, x, y);
            let actual = &scale_pixels[((y * 26 + x) * 4) as usize..][..4];
            for (actual, expected) in actual.iter().zip(expected) {
                assert!((*actual as i16 - expected as i16).abs() <= 2);
            }
        }
    }

    fn readback(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Vec<u8> {
        let unpadded = width * 4;
        let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("effect-test-readback"),
            size: u64::from(padded * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("effect-test-readback-encoder"),
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                sender.send(result).unwrap();
            });
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        receiver.recv().unwrap().unwrap();
        let mapped = buffer.slice(..).get_mapped_range();
        let mut result = Vec::with_capacity((width * height * 4) as usize);
        for row in mapped.chunks_exact(padded as usize).take(height as usize) {
            result.extend_from_slice(&row[..unpadded as usize]);
        }
        drop(mapped);
        buffer.unmap();
        result
    }

    fn bilinear(
        src: &[u8],
        width: u32,
        height: u32,
        output_width: u32,
        output_height: u32,
        x: u32,
        y: u32,
    ) -> [u8; 4] {
        let sx = ((x as f32 + 0.5) * width as f32 / output_width as f32) - 0.5;
        let sy = ((y as f32 + 0.5) * height as f32 / output_height as f32) - 0.5;
        let x0 = sx.floor().clamp(0.0, (width - 1) as f32) as u32;
        let y0 = sy.floor().clamp(0.0, (height - 1) as f32) as u32;
        let x1 = (x0 + 1).min(width - 1);
        let y1 = (y0 + 1).min(height - 1);
        let fx = sx.fract().clamp(0.0, 1.0);
        let fy = sy.fract().clamp(0.0, 1.0);
        let at = |xx, yy| &src[((yy * width + xx) * 4) as usize..][..4];
        std::array::from_fn(|channel| {
            let top = at(x0, y0)[channel] as f32 * (1.0 - fx) + at(x1, y0)[channel] as f32 * fx;
            let bottom = at(x0, y1)[channel] as f32 * (1.0 - fx) + at(x1, y1)[channel] as f32 * fx;
            (top * (1.0 - fy) + bottom * fy).round() as u8
        })
    }
}
