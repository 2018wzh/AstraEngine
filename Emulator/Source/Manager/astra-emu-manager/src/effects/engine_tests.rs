#[cfg(test)]
mod tests {
    use super::super::*;
    use std::{collections::BTreeMap, path::PathBuf};

    #[ignore = "requires the pinned local DXC DLL and a GPU adapter"]
    #[test]
    fn anime_pixels_match_pinned_reference_and_retain_on_invalid_reload() {
        let dxc = std::env::var_os("ASTRA_EMU_DXC_PATH")
            .map(PathBuf::from)
            .expect("ASTRA_EMU_DXC_PATH must point to the pinned DXC DLL");
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
                Some(&output),
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
        // Independent CPU evaluation of the pinned upstream GLSL, with FP16
        // intermediate storage and clamp-to-edge sampling. Samples cross the
        // 8/16-pixel dispatch boundaries and asymmetric image edges.
        assert_reference(
            &anime_chain_pixels,
            34,
            &[
                (0, 0, [0, 0, 0, 255]),
                (1, 0, [2, 0, 3, 255]),
                (33, 0, [156, 58, 0, 255]),
                (0, 1, [0, 1, 0, 255]),
                (7, 7, [46, 58, 78, 255]),
                (8, 8, [52, 67, 89, 255]),
                (9, 8, [57, 69, 98, 255]),
                (15, 15, [101, 130, 174, 255]),
                (16, 16, [106, 137, 184, 255]),
                (17, 19, [118, 160, 204, 255]),
                (33, 19, [204, 200, 81, 255]),
                (0, 37, [55, 235, 127, 255]),
                (17, 37, [137, 12, 3, 255]),
                (33, 37, [229, 57, 141, 255]),
            ],
        );
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
            .apply(&device, &queue, &input, Some(&output), &anime_config)
            .unwrap();
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let retained_pixels = readback(&device, &queue, &output, 34, 38);
        assert_eq!(retained_pixels, anime_chain_pixels);
        let external_source =
            include_str!("../../../../../Assets/Effects/Anime4K/upscale_cnn_x2_s.hlsl");
        let external_restore =
            include_str!("../../../../../Assets/Effects/Anime4K/restore_cnn_s.hlsl");
        engine
            .reload_source(&device, external_restore, &anime_config)
            .unwrap();
        assert_eq!(engine.active_output_dimensions(17, 19).unwrap(), (17, 19));
        let restore_output = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("effect-test-external-restore-output"),
            size: wgpu::Extent3d {
                width: 17,
                height: 19,
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
                Some(&restore_output),
                &anime_config,
            )
            .unwrap();
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let restored_pixels = readback(&device, &queue, &restore_output, 17, 19);
        assert_reference(
            &restored_pixels,
            17,
            &[
                (0, 0, [0, 0, 0, 255]),
                (1, 0, [9, 2, 15, 255]),
                (16, 0, [161, 63, 1, 255]),
                (0, 1, [2, 11, 6, 255]),
                (7, 7, [98, 125, 168, 255]),
                (8, 8, [111, 143, 191, 255]),
                (9, 8, [124, 150, 210, 255]),
                (15, 15, [222, 27, 116, 255]),
                (16, 16, [220, 29, 128, 255]),
                (8, 9, [112, 153, 196, 255]),
                (16, 9, [202, 196, 78, 255]),
                (0, 18, [54, 234, 126, 255]),
                (8, 18, [129, 4, 0, 255]),
                (16, 18, [228, 56, 140, 255]),
            ],
        );
        engine
            .reload_source(&device, external_source, &anime_config)
            .unwrap();
        assert_eq!(engine.active_output_dimensions(17, 19).unwrap(), (34, 38));
        engine
            .apply(&device, &queue, &input, Some(&output), &anime_config)
            .unwrap();
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let external_pixels = readback(&device, &queue, &output, 34, 38);
        assert_reference(
            &external_pixels,
            34,
            &[
                (0, 0, [1, 1, 1, 255]),
                (1, 0, [4, 2, 5, 255]),
                (33, 0, [179, 83, 19, 255]),
                (0, 1, [0, 3, 1, 255]),
                (7, 7, [46, 59, 79, 255]),
                (8, 8, [53, 68, 90, 255]),
                (9, 8, [58, 70, 98, 255]),
                (15, 15, [102, 131, 174, 255]),
                (16, 16, [109, 140, 186, 255]),
                (17, 19, [118, 161, 204, 255]),
                (33, 19, [204, 201, 81, 255]),
                (0, 37, [54, 234, 126, 255]),
                (17, 37, [156, 31, 22, 255]),
                (33, 37, [231, 59, 143, 255]),
            ],
        );
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
        assert_eq!(engine.active_output_dimensions(17, 19).unwrap(), (26, 29));
        engine
            .apply(&device, &queue, &input, Some(&scale_output), &scale_config)
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

    fn assert_reference(pixels: &[u8], width: usize, samples: &[(usize, usize, [u8; 4])]) {
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 255)
        );
        for &(x, y, expected) in samples {
            let actual = &pixels[(y * width + x) * 4..][..4];
            for (channel, (&actual, expected)) in actual.iter().zip(expected).enumerate() {
                assert!(
                    actual.abs_diff(expected) <= 3,
                    "pixel ({x},{y}) channel {channel}: {actual}, expected {expected}"
                );
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
