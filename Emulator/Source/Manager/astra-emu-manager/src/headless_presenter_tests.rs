use super::*;
use astra_emu_manager_core::model::{
    allocate_session_epoch, ConfigEntry, ConfigValue, FrameIdentity, FramePoll, WindowState,
};
use astra_emu_manager_core::{
    no_host_services, LoadedFamilyPlugin, NativeCancellation, NativeOpenRequest,
};
use std::time::Instant;

// Public synthetic HCB: native modal UI over a black scene, then native ThreadNext.
fn fixture(modal: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let mut bytes = vec![0, 0, 0, 0, 3, 0, 0, 3, 1, 0, 6, 7, 0, 0, 0];
    let descriptor = bytes.len() as u32;
    bytes[..4].copy_from_slice(&descriptor.to_le_bytes());
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&3u16.to_le_bytes());
    bytes.extend_from_slice(&[0, 0, 1, 0]);
    bytes.extend_from_slice(&3u16.to_le_bytes());
    for (name, arguments) in [(modal, 0), ("ThreadNext", 0), ("ConfigEtc", 3)] {
        bytes.extend_from_slice(&[arguments, name.len() as u8 + 1]);
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
    }
    bytes.extend_from_slice(&0u16.to_le_bytes());
    std::fs::write(root.path().join("fixture.hcb"), bytes).unwrap();
    root
}
fn request(
    root: &std::path::Path,
    surface: &str,
    host: Option<astra_emu_abi::D3D12Host>,
) -> NativeOpenRequest {
    let epoch = allocate_session_epoch().unwrap();
    let cancellation = NativeCancellation::new(epoch);
    NativeOpenRequest {
        game_path: root.to_string_lossy().into(),
        session_epoch: epoch,
        configuration: vec![
            ConfigEntry {
                id: "surface".into(),
                value: ConfigValue::Enum(surface.into()),
            },
            ConfigEntry {
                id: "render_scale".into(),
                value: ConfigValue::Number(0.5),
            },
        ],
        initial_window: WindowState {
            width: 800,
            height: 600,
            focused: true,
            visible: true,
            scale_factor: 1.0,
        },
        host: no_host_services(&cancellation),
        cancellation,
        d3d12: host,
    }
}
fn next(
    session: &mut astra_emu_manager_core::NativeSession,
    after: Option<FrameIdentity>,
) -> FrameLease {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let state = session.state().unwrap();
        assert_ne!(
            state.state,
            astra_emu_abi::SESSION_FAILED,
            "{:?}",
            state.error
        );
        if let FramePoll::Ready(frame) = session.acquire_frame(after).unwrap() {
            return frame;
        }
        assert!(Instant::now() < deadline, "native frame timeout");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
#[ignore = "requires built RFVP DLL, hardware D3D12 and native audio"]
fn rfvp_d3d12_production_presenter_pixels_leases_cancel_reopen() {
    let dll = std::env::var_os("ASTRA_EMU_RFVP_D3D12_PLUGIN").expect("set RFVP DLL path");
    let mut plugin = LoadedFamilyPlugin::load(dll).unwrap();
    for modal in ["LoadFile", "ExitDialog"] {
        let root = fixture(modal);
        // Explicit native D3D12 CPU baseline; no Vulkan/DX12 backend conflation.
        let mut cpu = plugin
            .open(request(root.path(), "cpu_d3d12", None))
            .unwrap();
        let mut baseline = next(&mut cpu, None);
        for _ in 0..100 {
            if baseline
                .pixels()
                .unwrap()
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[0] > 32 || p[1] > 32 || p[2] > 32)
            {
                break;
            }
            baseline = next(&mut cpu, Some(baseline.identity));
        }
        let expected = super::super::capture_image(&baseline).unwrap();
        assert!(expected
            .pixels()
            .any(|p| p[0] > 32 || p[1] > 32 || p[2] > 32));
        cpu.close().unwrap();
        for cycle in 0..3 {
            let epoch = allocate_session_epoch().unwrap();
            let mut presenter = Presenter::new(epoch)
                .unwrap()
                .expect("hardware D3D12 required");
            let host = presenter.host().unwrap().unwrap();
            // Requested epochs belong to each open, so activate the actual one.
            let request = request(root.path(), "d3d12", Some(host));
            presenter.mailbox.activate(request.session_epoch).unwrap();
            let mut session = plugin.open(request).unwrap();
            let mut frame = next(&mut session, None);
            assert!(frame.pixels().is_none());
            let mut actual;
            for _ in 0..100 {
                presenter.present(&frame).unwrap();
                actual = presenter.capture().unwrap();
                if actual.pixels().any(|p| p[0] > 32 || p[1] > 32 || p[2] > 32) {
                    break;
                }
                frame = next(&mut session, Some(frame.identity));
            }
            actual = presenter.capture().unwrap();
            assert_eq!(actual.dimensions(), (320, 240));
            assert_eq!(
                actual.as_raw(),
                expected.as_raw(),
                "{modal} cycle {cycle}: full UI pixels differ"
            );
            let source = presenter.capture_native_source(&frame).unwrap();
            let reference =
                FrameLease::from_vec(source.into_raw(), frame.info, frame.identity).unwrap();
            let same_frame_cpu = super::super::capture_image(&reference).unwrap();
            assert_eq!(
                actual.as_raw(),
                same_frame_cpu.as_raw(),
                "same-generation CPU normalization versus production GPU presentation"
            );
            let _slint_image =
                slint::Image::try_from(presenter.output.as_ref().unwrap().texture.clone())
                    .expect("production GPU output must import into Slint");
            let old = frame.clone();
            let second = next(&mut session, Some(frame.identity));
            let third = next(&mut session, Some(second.identity));
            std::thread::sleep(Duration::from_millis(75));
            assert!(
                !matches!(
                    session.acquire_frame(Some(third.identity)).unwrap(),
                    FramePoll::Ready(_)
                ),
                "three-slot pool must remain bounded"
            );
            assert_eq!(
                old.d3d12().unwrap().descriptor().identity.generation,
                old.identity.generation
            );
            // Window geometry is Host-only; native R stays fixed.
            session
                .input(
                    &[
                        astra_emu_manager_core::model::FamilyEvent::WindowResized {
                            width: 1733,
                            height: 977,
                        },
                        astra_emu_manager_core::model::FamilyEvent::WindowScale { factor: 1.5 },
                    ],
                    Some(third.identity),
                )
                .unwrap();
            presenter
                .renderer
                .configure_filter(
                    &astra_emu_manager::effects::FilterConfiguration {
                        preset: astra_emu_manager::effects::FilterPreset::Scale,
                        scale: 2.0,
                        ..Default::default()
                    },
                    None,
                )
                .unwrap();
            presenter.present(&frame).unwrap();
            assert_eq!(presenter.capture().unwrap().dimensions(), (640, 480));
            presenter
                .renderer
                .configure_filter(&Default::default(), None)
                .unwrap();
            presenter.present(&frame).unwrap();
            assert_eq!(presenter.capture().unwrap().as_raw(), expected.as_raw());
            session
                .input(
                    &[astra_emu_manager_core::model::FamilyEvent::Key {
                        code: astra_emu_manager_core::model::KeyCode::Escape,
                        state: astra_emu_manager_core::model::KeyState::Pressed,
                        modifiers: astra_emu_manager_core::model::KeyModifiers {
                            shift: false,
                            control: false,
                            alt: false,
                            super_key: false,
                        },
                        logical: Some("Escape".into()),
                    }],
                    Some(third.identity),
                )
                .unwrap();
            drop(second);
            drop(old);
            drop(frame);
            let mut response = next(&mut session, Some(third.identity));
            for _ in 0..100 {
                presenter.present(&response).unwrap();
                if presenter
                    .capture()
                    .unwrap()
                    .pixels()
                    .all(|p| p.0[..3] == [0, 0, 0])
                {
                    break;
                }
                response = next(&mut session, Some(response.identity));
            }
            assert_eq!((response.info.width, response.info.height), (320, 240));
            assert!(
                presenter
                    .capture()
                    .unwrap()
                    .pixels()
                    .all(|p| p.0[..3] == [0, 0, 0]),
                "Escape must dismiss native UI"
            );
            let close = Instant::now();
            session.cancel();
            session.close().unwrap();
            assert!(close.elapsed() < Duration::from_secs(5));
            // This was never imported before close; its independent lease must
            // still open handles and deliver intact final UI after App destruction.
            presenter
                .mailbox
                .activate(third.identity.session_epoch)
                .unwrap();
            presenter.present(&third).unwrap();
            assert_eq!(presenter.capture().unwrap().as_raw(), expected.as_raw());
            drop(third);
            drop(response);
            presenter.close().unwrap();
            println!("D3D12 {modal} cycle={cycle} L=640x480 R=320x240 O=320x240 bytes_per_GPU_copy={} delivery_CPU_bytes=0 slots=3", 320 * 240 * 4);
        }
        // Negotiation rejects absent/mismatched Host before publishing a session.
        assert!(plugin.open(request(root.path(), "d3d12", None)).is_err());
        let probe = Presenter::new(allocate_session_epoch().unwrap())
            .unwrap()
            .unwrap();
        let mut wrong = probe.host().unwrap().unwrap();
        wrong.adapter_luid ^= u64::MAX;
        assert!(plugin
            .open(request(root.path(), "d3d12", Some(wrong)))
            .is_err());
    }
    plugin.shutdown().unwrap();
}

#[test]
#[ignore = "requires built RFVP DLL, hardware D3D12 and native audio"]
fn rfvp_d3d12_bidirectional_fence_does_not_block_native_close() {
    use windows::Win32::{Foundation::HANDLE, Graphics::Direct3D12::*};
    struct Gate(ID3D12Fence);
    impl Drop for Gate {
        fn drop(&mut self) {
            unsafe {
                let _ = self.0.Signal(1);
            }
        }
    }
    let dll = std::env::var_os("ASTRA_EMU_RFVP_D3D12_PLUGIN").unwrap();
    let mut plugin = LoadedFamilyPlugin::load(dll).unwrap();
    let root = fixture("LoadFile");
    let mut presenter = Presenter::new(allocate_session_epoch().unwrap())
        .unwrap()
        .unwrap();
    let opening = request(root.path(), "d3d12", presenter.host().unwrap());
    presenter.mailbox.activate(opening.session_epoch).unwrap();
    let mut session = plugin.open(opening).unwrap();
    let first = next(&mut session, None);
    let (device, queue) = {
        let hal = unsafe { presenter.device.as_hal::<wgpu::hal::api::Dx12>() }.unwrap();
        (hal.raw_device().clone(), hal.raw_queue().clone())
    };
    let gate = Gate(unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.unwrap());
    unsafe {
        queue.Wait(&gate.0, 1).unwrap();
    }
    presenter.present(&first).unwrap();
    let descriptor = first.d3d12().unwrap().descriptor();
    let mut done: Option<ID3D12Fence> = None;
    unsafe {
        device
            .OpenSharedHandle(
                HANDLE(descriptor.lease.done_fence_handle as *mut _),
                &mut done,
            )
            .unwrap();
    }
    let done = done.unwrap();
    assert!(unsafe { done.GetCompletedValue() } < descriptor.lease.done_value);
    let second = next(&mut session, Some(first.identity));
    let third = next(&mut session, Some(second.identity));
    std::thread::sleep(Duration::from_millis(75));
    assert!(!matches!(
        session.acquire_frame(Some(third.identity)).unwrap(),
        FramePoll::Ready(_)
    ));
    let started = Instant::now();
    session.cancel();
    session.close().unwrap();
    let close_ms = started.elapsed().as_millis();
    assert!(close_ms < 5000, "native close waited on Host consumption");
    assert!(unsafe { done.GetCompletedValue() } < descriptor.lease.done_value);
    unsafe {
        gate.0.Signal(1).unwrap();
    }
    let captured = presenter.capture().unwrap();
    assert_eq!(captured.dimensions(), (320, 240));
    assert!(unsafe { done.GetCompletedValue() } >= descriptor.lease.done_value);
    println!("D3D12 blocked Host fence={} completed={} native_close_ms={close_ms} slots=3 delivery_CPU_bytes=0",
        descriptor.lease.done_value, unsafe { done.GetCompletedValue() });
    presenter.close().unwrap();
    drop(first);
    drop(second);
    drop(third);
    let mut reopened = plugin
        .open(request(root.path(), "cpu_d3d12", None))
        .unwrap();
    reopened.close().unwrap();
    plugin.shutdown().unwrap();
}

#[test]
#[ignore = "isolated process only: deliberately removes the owned Host D3D12 device"]
fn rfvp_d3d12_device_loss_reports_failed_cleanup_and_blocks_reopen() {
    use windows::{core::Interface, Win32::Graphics::Direct3D12::ID3D12Device5};
    let dll = std::env::var_os("ASTRA_EMU_RFVP_D3D12_PLUGIN").unwrap();
    let mut plugin = LoadedFamilyPlugin::load(dll).unwrap();
    let root = fixture("ExitDialog");
    let mut presenter = Presenter::new(allocate_session_epoch().unwrap())
        .unwrap()
        .unwrap();
    let opening = request(root.path(), "d3d12", presenter.host().unwrap());
    presenter.mailbox.activate(opening.session_epoch).unwrap();
    let mut session = plugin.open(opening).unwrap();
    let frame = next(&mut session, None);
    presenter.present(&frame).unwrap();
    let device: ID3D12Device5 = unsafe { presenter.device.as_hal::<wgpu::hal::api::Dx12>() }
        .unwrap()
        .raw_device()
        .cast()
        .unwrap();
    unsafe {
        device.RemoveDevice();
    }
    assert!(
        presenter.close().is_err(),
        "device removal must not count as successful graphics cleanup"
    );
    session.cancel();
    let started = Instant::now();
    let result = session.close();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(Presenter::new(allocate_session_epoch().unwrap()).is_err());
    println!(
        "D3D12 explicit owned-device removal: native_cleanup={result:?}, Host reopen rejected"
    );
    plugin.shutdown().unwrap();
}
