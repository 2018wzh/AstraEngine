use super::*;
use astra_emu_manager::effects::FilterPreset;
use astra_emu_manager_core::model::{
    FrameAlpha, FrameFormat, FrameInfo, FrameLease, FrameRowOrder,
};
use std::{collections::BTreeMap, path::PathBuf};

fn info(width: u32, height: u32, stride: u32, alpha: FrameAlpha) -> FrameInfo {
    FrameInfo {
        width,
        height,
        logical_width: width,
        logical_height: height,
        stride,
        format: FrameFormat::Rgba8Srgb { alpha },
        rows: FrameRowOrder::TopToBottom,
    }
}

fn lease(pixels: Vec<u8>, info: FrameInfo, epoch: u64, generation: u64) -> FrameLease {
    FrameLease::from_vec(
        pixels,
        info,
        FrameIdentity {
            session_epoch: epoch,
            output_epoch: 1,
            generation,
        },
    )
    .unwrap()
}

#[test]
fn old_frame_and_shutdown_cannot_replace_or_clear_a_new_session_baseline() {
    let mailbox = FrameMailbox::new();
    mailbox.activate(1).unwrap();
    let old = lease(vec![1, 2, 3, 255], info(1, 1, 4, FrameAlpha::Opaque), 1, 7);
    mailbox.publish(old.clone()).unwrap();
    mailbox.clear(1).unwrap();
    mailbox.activate(2).unwrap();
    mailbox
        .publish(lease(
            vec![5, 6, 7, 255],
            info(1, 1, 4, FrameAlpha::Opaque),
            2,
            1,
        ))
        .unwrap();
    assert!(mailbox.publish(old).is_err());
    mailbox.clear(1).unwrap();
    let current = mailbox.snapshot().unwrap().unwrap();
    assert_eq!(
        current.identity,
        FrameIdentity {
            session_epoch: 2,
            output_epoch: 1,
            generation: 1
        }
    );
    assert_eq!(current.pixels(), &[5, 6, 7, 255]);
}

#[test]
fn replacement_and_close_preserve_an_already_acquired_cpu_lease() {
    let mailbox = FrameMailbox::new();
    mailbox.activate(3).unwrap();
    mailbox
        .publish(lease(
            vec![3, 4, 5, 255],
            info(1, 1, 4, FrameAlpha::Opaque),
            3,
            1,
        ))
        .unwrap();
    let held = mailbox.snapshot().unwrap().unwrap();
    mailbox
        .publish(lease(
            vec![7, 8, 9, 255],
            info(1, 1, 4, FrameAlpha::Opaque),
            3,
            2,
        ))
        .unwrap();
    mailbox.clear(3).unwrap();
    assert!(mailbox.snapshot().unwrap().is_none());
    assert_eq!(held.pixels(), &[3, 4, 5, 255]);
}

#[test]
fn filtered_output_keeps_the_complete_native_frame_interactive() {
    let content = super::content_viewport_for_output(1280, 720, 721, 1280, 1082, 1920).unwrap();
    assert_eq!(
        content,
        astra_media_core::Viewport2D {
            x: 0,
            y: 0,
            width: 1082,
            height: 1920,
        }
    );

    let content = super::content_viewport_for_output(1280, 720, 1920, 1200, 2880, 1800).unwrap();
    assert_eq!(
        content,
        astra_media_core::Viewport2D {
            x: 0,
            y: 0,
            width: 2880,
            height: 1800,
        }
    );
}

fn read_pixels(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
    let stride = (texture.width() * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("stage-test-readback"),
        size: u64::from(stride) * u64::from(texture.height()),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(texture.height()),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap()
        });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receiver.recv().unwrap().unwrap();
    let mapped = buffer.slice(..).get_mapped_range();
    mapped
        .chunks(stride as usize)
        .flat_map(|row| row[..texture.width() as usize * 4].iter().copied())
        .collect()
}

#[ignore = "requires a GPU adapter and the pinned local DXC DLL"]
#[test]
fn filter_configuration_rolls_back_and_reuses_uploaded_input() {
    let dxc_path = std::env::var_os("ASTRA_EMU_DXC_PATH")
        .map(PathBuf::from)
        .expect("ASTRA_EMU_DXC_PATH must point to the pinned DXC DLL");
    assert!(dxc_path.is_file());
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::VULKAN;
    let instance = wgpu::Instance::new(descriptor);
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("a GPU adapter is required for this test");
    assert!(matches!(
        adapter.get_info().device_type,
        wgpu::DeviceType::DiscreteGpu | wgpu::DeviceType::IntegratedGpu
    ));
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: FilterEngine::required_features(),
        ..Default::default()
    }))
    .expect("GPU device creation must succeed");
    let mailbox = FrameMailbox::new();
    mailbox.activate(1).unwrap();
    let mut renderer = ManagerStageRenderer::with_dxc_path(mailbox.clone(), dxc_path);
    renderer
        .setup(WgpuFrameContext {
            device: &device,
            queue: &queue,
        })
        .unwrap();
    let config = FilterConfiguration {
        preset: FilterPreset::Scale,
        scale: 2.0,
        strength: 0.35,
        parameters: BTreeMap::new(),
    };
    renderer.configure_filter(&config, None).unwrap();
    let old_output = (renderer.output_width, renderer.output_height);
    let invalid = "//!VERSION 4\nthis is not an effect";
    assert!(renderer.configure_filter(&config, Some(invalid)).is_err());
    assert_eq!(renderer.filter_configuration, config);
    assert_eq!((renderer.output_width, renderer.output_height), old_output);

    mailbox
        .publish(lease(
            vec![1, 2, 3, 255],
            info(1, 1, 4, FrameAlpha::Opaque),
            1,
            1,
        ))
        .unwrap();
    renderer
        .render(WgpuFrameContext {
            device: &device,
            queue: &queue,
        })
        .unwrap();
    let uploaded_identity = renderer.uploaded_identity;
    assert_eq!(
        read_pixels(&device, &queue, renderer.output_texture.as_ref().unwrap()),
        [1, 2, 3, 255].repeat(4)
    );
    // Change only the GPU input: reconfiguration must not overwrite it with
    // the older CPU mailbox frame when the generation is unchanged.
    queue.write_texture(
        renderer.input_texture.as_ref().unwrap().as_image_copy(),
        &[200, 60, 10, 255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    renderer
        .configure_filter(&FilterConfiguration::default(), None)
        .unwrap();
    renderer
        .render(WgpuFrameContext {
            device: &device,
            queue: &queue,
        })
        .unwrap();
    assert_eq!(renderer.uploaded_identity, uploaded_identity);
    assert_eq!(
        read_pixels(&device, &queue, renderer.output_texture.as_ref().unwrap()),
        [200, 60, 10, 255]
    );
    assert_eq!(
        renderer.output_texture.as_ref(),
        renderer.input_texture.as_ref()
    );
    assert!(renderer.filtered_texture.is_none());
    assert!(renderer.configure_filter(&config, Some(invalid)).is_err());
    renderer
        .render(WgpuFrameContext {
            device: &device,
            queue: &queue,
        })
        .unwrap();
    assert_eq!(
        read_pixels(&device, &queue, renderer.output_texture.as_ref().unwrap()),
        [200, 60, 10, 255]
    );
    mailbox.clear(1).unwrap();
    renderer
        .render(WgpuFrameContext {
            device: &device,
            queue: &queue,
        })
        .unwrap();
    assert_eq!(
        read_pixels(&device, &queue, renderer.output_texture.as_ref().unwrap()),
        [0, 0, 0, 0]
    );
    assert!(renderer.input_texture.is_none());
}

#[test]
#[ignore = "requires a native hardware GPU runner"]
fn native_pixels_keep_row_padding_and_present_bgra_linear_ignored_alpha_after_recovery() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .unwrap();
    assert_ne!(adapter.get_info().device_type, wgpu::DeviceType::Cpu);
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let mailbox = FrameMailbox::new();
    mailbox.activate(7).unwrap();
    let frame = lease(
        vec![
            128, 0, 0, 0, 0, 128, 0, 1, 42, 42, 42, 42, 0, 0, 128, 7, 128, 128, 128, 17, 42, 42,
            42, 42,
        ],
        FrameInfo {
            width: 2,
            height: 2,
            logical_width: 2,
            logical_height: 2,
            stride: 12,
            format: FrameFormat::Bgra8Linear {
                alpha: FrameAlpha::Ignored,
            },
            rows: FrameRowOrder::BottomToTop,
        },
        7,
        1,
    );
    mailbox.publish(frame).unwrap();
    let mut renderer = ManagerStageRenderer::with_dxc_path(mailbox.clone(), PathBuf::new());
    let expected = [
        188u8, 0, 0, 255, 188, 188, 188, 255, 0, 0, 188, 255, 0, 188, 0, 255,
    ];
    for _ in 0..2 {
        renderer
            .setup(WgpuFrameContext {
                device: &device,
                queue: &queue,
            })
            .unwrap();
        renderer
            .render(WgpuFrameContext {
                device: &device,
                queue: &queue,
            })
            .unwrap();
        let actual = read_pixels(&device, &queue, renderer.output_texture.as_ref().unwrap());
        assert!(actual
            .iter()
            .zip(expected)
            .all(|(value, expected)| value.abs_diff(expected) <= 1));
        renderer.teardown();
    }
    // Rebuilding the GPU does not discard the immutable complete CPU baseline.
    assert_eq!(
        mailbox.snapshot().unwrap().unwrap().identity,
        FrameIdentity {
            session_epoch: 7,
            output_epoch: 1,
            generation: 1
        }
    );
}
