use super::*;
use astra_emu_family_api::FrameInfo;
use astra_emu_manager::effects::FilterPreset;
use std::{collections::BTreeMap, path::PathBuf};

fn info(width: u32, height: u32, stride: u32, alpha: FrameAlpha) -> FrameInfo {
    FrameInfo {
        width,
        height,
        stride,
        format: FrameFormat::Rgba8Srgb { alpha },
    }
}

#[test]
fn frame_copy_strips_source_padding_and_aligns_upload_rows() {
    let mailbox = FrameMailbox::new();
    let source = [1_u8, 2, 3, 255, 5, 6, 7, 255, 99, 99, 99, 99];
    mailbox
        .publish(FrameView::from_slice(&source, info(2, 1, 12, FrameAlpha::Opaque)).unwrap())
        .unwrap();
    let frame = mailbox.snapshot().unwrap().unwrap();
    assert_eq!(frame.stride, 256);
    assert_eq!(&frame.pixels[..8], &[1, 2, 3, 255, 5, 6, 7, 255]);
    assert!(frame.pixels[8..].iter().all(|value| *value == 0));
}

#[test]
fn clear_removes_the_previous_session_frame() {
    let mailbox = FrameMailbox::new();
    mailbox
        .publish(FrameView::from_slice(&[1, 2, 3, 255], info(1, 1, 4, FrameAlpha::Opaque)).unwrap())
        .unwrap();
    let generation = mailbox.snapshot().unwrap().unwrap().generation;
    mailbox.clear().unwrap();
    assert!(mailbox.snapshot().unwrap().is_none());
    mailbox
        .publish(FrameView::from_slice(&[5, 6, 7, 255], info(1, 1, 4, FrameAlpha::Opaque)).unwrap())
        .unwrap();
    assert!(mailbox.snapshot().unwrap().unwrap().generation > generation);
}

#[test]
fn collector_publishes_only_the_last_synchronous_frame() {
    let mailbox = FrameMailbox::new();
    let mut collector = FrameCollector::new(mailbox.clone());
    collector
        .accept(FrameView::from_slice(&[3, 4, 5, 255], info(1, 1, 4, FrameAlpha::Opaque)).unwrap())
        .unwrap();
    collector
        .accept(FrameView::from_slice(&[7, 8, 9, 255], info(1, 1, 4, FrameAlpha::Opaque)).unwrap())
        .unwrap();
    let frame = mailbox.snapshot().unwrap().unwrap();
    assert_eq!(&frame.pixels[..4], &[7, 8, 9, 255]);
    assert_eq!(frame.generation, 2);
}

#[test]
fn invalid_opaque_alpha_preserves_previous_frame() {
    let mailbox = FrameMailbox::new();
    mailbox
        .publish(FrameView::from_slice(&[1, 2, 3, 255], info(1, 1, 4, FrameAlpha::Opaque)).unwrap())
        .unwrap();
    let previous = mailbox.snapshot().unwrap().unwrap();
    assert!(mailbox
        .publish(FrameView::from_slice(&[1, 2, 3, 0], info(1, 1, 4, FrameAlpha::Opaque)).unwrap())
        .is_err());
    let current = mailbox.snapshot().unwrap().unwrap();
    assert_eq!(current.generation, previous.generation);
    assert_eq!(current.pixels, previous.pixels);
}

fn read_pixels(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
    let stride = align_row(texture.width() * 4).unwrap();
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
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: FilterEngine::required_features(),
        ..Default::default()
    }))
    .expect("GPU device creation must succeed");
    let mailbox = FrameMailbox::new();
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
        .publish(FrameView::from_slice(&[1, 2, 3, 255], info(1, 1, 4, FrameAlpha::Opaque)).unwrap())
        .unwrap();
    renderer
        .render(WgpuFrameContext {
            device: &device,
            queue: &queue,
        })
        .unwrap();
    let uploaded_generation = renderer.uploaded_generation;
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
    assert_eq!(renderer.uploaded_generation, uploaded_generation);
    assert_eq!(
        read_pixels(&device, &queue, renderer.output_texture.as_ref().unwrap()),
        [200, 60, 10, 255]
    );
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
    mailbox.clear().unwrap();
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
