use astra_platform::{PlatformCapabilityReport, PlatformId, ReportBackedPlatformHost, SdkStatus};

pub fn host(target: Option<&str>) -> ReportBackedPlatformHost {
    ReportBackedPlatformHost::new(probe(target))
}

pub fn probe(target: Option<&str>) -> PlatformCapabilityReport {
    tracing::info!(
        event = "platform.probe.start",
        platform = "macos",
        has_target = target.is_some(),
        "platform capability probe started"
    );
    PlatformCapabilityReport::new(
        PlatformId::Macos,
        target.map(str::to_string),
        if cfg!(target_os = "macos") {
            SdkStatus::Present
        } else {
            SdkStatus::Missing
        },
        vec!["wgpu_metal".to_string(), "headless".to_string()],
        vec!["avfoundation".to_string()],
        vec!["coreaudio".to_string()],
        vec!["app_support".to_string(), "file_package".to_string()],
        vec![
            "keyboard".to_string(),
            "mouse".to_string(),
            "ime".to_string(),
            "gamepad".to_string(),
        ],
        vec![
            "appkit".to_string(),
            "resize".to_string(),
            "crash_bundle".to_string(),
        ],
        vec!["network_runtime_ai_profile_gated".to_string()],
    )
}
