use std::sync::Arc;

use astra_headless_protocol::{ButtonState, PhysicalInput, PointerButton};
use astra_headless_vn_adapter::NativeVnProductAdapterFactory;
use astra_platform::{HeadlessHostProfile, PlatformHostFactory};
use astra_platform_headless::HeadlessPlatformFactory;
use astra_product_host::{ProductAdapterFactory, ProductOpenRequest, ProductPerformanceObserver};

struct TestPerformanceObserver;

impl ProductPerformanceObserver for TestPerformanceObserver {
    fn record_phase(&self, _name: &str) -> Result<(), String> {
        Ok(())
    }
}

#[astra_headless_test::tokio_test]
async fn real_native_vn_package_accepts_physical_input_and_produces_cpu_frame() {
    let package = astra_player_vn::headless_test_fixture::product_package(
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:line speaker:hero #@id line.one\n    choice key:choice.next #@id choice.next\n      option key:choice.end -> ending #@id choice.end\nstate ending #@id state.ending\n  scene room #@id scene.ending\n    text key:line.after speaker:hero #@id line.ending\n    system_page kind:backlog policy:astra.policy.standard #@id page.backlog\n",
    );
    let package_hash = astra_core::Hash256::from_sha256(&package).to_string();
    let root = tempfile::tempdir().unwrap();
    let mut profile = HeadlessHostProfile::reference(
        "nativevn-game",
        "com.example.player.audio",
        astra_core::Hash256::from_sha256(b"test-build").to_string(),
        package_hash,
    );
    profile.viewport_width = 320;
    profile.viewport_height = 180;
    let max_video_frames = profile.max_video_frames;
    let max_decode_output_bytes = profile.max_decode_output_bytes;
    let max_decoded_cache_bytes = profile.max_decoded_cache_bytes;
    let host = HeadlessPlatformFactory::new(root.path(), root.path())
        .start(profile.into())
        .await
        .unwrap();
    let mut product = NativeVnProductAdapterFactory::default()
        .open(ProductOpenRequest {
            package: astra_product_host::ProductPackageSource::InMemory(Arc::from(package)),
            profile: "classic".into(),
            target: "nativevn-game".into(),
            locale: Some("en".into()),
            width: 320,
            height: 180,
            max_video_frames,
            max_decode_output_bytes,
            max_decoded_cache_bytes,
            retain_audio_timeline: true,
            performance_observer: Some(Arc::new(TestPerformanceObserver)),
            presentation_rate_hz: astra_platform::HEADLESS_PRESENTATION_RATE_HZ,
            platform: host.client.clone(),
        })
        .await
        .unwrap();
    assert!(product
        .consume(
            0,
            &PhysicalInput::Keyboard {
                physical_key: "Enter".into(),
                logical_key: Some("Enter".into()),
                state: ButtonState::Pressed,
                repeat: false,
            },
        )
        .await
        .is_err());
    product.consume(0, &PhysicalInput::Resume).await.unwrap();
    product
        .consume(0, &PhysicalInput::Focus { focused: true })
        .await
        .unwrap();
    let observations = product
        .consume(
            1,
            &PhysicalInput::Keyboard {
                physical_key: "Enter".into(),
                logical_key: Some("Enter".into()),
                state: ButtonState::Pressed,
                repeat: false,
            },
        )
        .await
        .unwrap();
    assert!(observations
        .iter()
        .any(|item| item.key == "runtime.state_hash"));
    product
        .consume(
            2,
            &PhysicalInput::Keyboard {
                physical_key: "Digit1".into(),
                logical_key: Some("1".into()),
                state: ButtonState::Pressed,
                repeat: false,
            },
        )
        .await
        .unwrap();
    product
        .consume(
            3,
            &PhysicalInput::PointerMove {
                x: 32_768,
                y: 32_768,
            },
        )
        .await
        .unwrap();
    product
        .consume(
            4,
            &PhysicalInput::PointerButton {
                button: PointerButton::Primary,
                state: ButtonState::Pressed,
            },
        )
        .await
        .unwrap();
    for (tick, key) in [(5, "KeyB"), (6, "Escape"), (7, "F5"), (8, "F9")] {
        product
            .consume(
                tick,
                &PhysicalInput::Keyboard {
                    physical_key: key.into(),
                    logical_key: None,
                    state: ButtonState::Pressed,
                    repeat: false,
                },
            )
            .await
            .unwrap();
    }
    let frame = product.capture_frame().await.unwrap();
    assert_eq!((frame.width, frame.height), (320, 180));
    assert!(frame.rgba8.iter().any(|byte| *byte != 0));
    let performance = product.take_performance_sample();
    assert!(
        performance.runtime_tick_ns
            + performance.vn_step_ns
            + performance.ui_layout_paint_ns
            + performance.media_decode_ns
            + performance.save_load_ns
            > 0
    );
    product.shutdown().await.unwrap();
    host.client.shutdown().await.unwrap();
    assert!(root.path().join("artifact-manifest.json").is_file());
}
