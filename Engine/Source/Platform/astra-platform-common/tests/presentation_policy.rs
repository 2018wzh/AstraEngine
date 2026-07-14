use astra_platform::PlatformEventKind;
use astra_platform_common::{wgpu_device_recovery_events, wgpu_recovery_events};

#[test]
fn successful_surface_recovery_emits_loss_before_restore() {
    assert_eq!(
        wgpu_recovery_events("webgpu", true),
        vec![
            PlatformEventKind::ContextLost {
                provider: "webgpu".to_string()
            },
            PlatformEventKind::ContextRestored {
                provider: "webgpu".to_string()
            },
        ]
    );
    assert_eq!(
        wgpu_recovery_events("webgpu", false),
        vec![PlatformEventKind::ContextLost {
            provider: "webgpu".to_string()
        }]
    );
}

#[test]
fn successful_device_recovery_emits_device_loss_before_restore() {
    assert_eq!(
        wgpu_device_recovery_events("wgpu_hardware", true),
        vec![
            PlatformEventKind::DeviceLost {
                provider: "wgpu_hardware".to_string()
            },
            PlatformEventKind::DeviceRestored {
                provider: "wgpu_hardware".to_string()
            },
        ]
    );
    assert_eq!(
        wgpu_device_recovery_events("wgpu_hardware", false),
        vec![PlatformEventKind::DeviceLost {
            provider: "wgpu_hardware".to_string()
        }]
    );
}
