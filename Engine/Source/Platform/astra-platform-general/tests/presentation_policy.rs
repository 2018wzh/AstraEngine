use astra_platform::PlatformEventKind;
use astra_platform_general::wgpu_recovery_events;

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
