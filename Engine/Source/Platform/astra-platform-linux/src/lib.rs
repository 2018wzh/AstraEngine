use astra_platform::{PlatformCapabilityReport, PlatformId, ReportBackedPlatformHost, SdkStatus};

pub fn host(target: Option<&str>) -> ReportBackedPlatformHost {
    ReportBackedPlatformHost::new(probe(target))
}

pub fn probe(target: Option<&str>) -> PlatformCapabilityReport {
    tracing::info!(
        event = "platform.probe.start",
        platform = "linux",
        has_target = target.is_some(),
        "platform capability probe started"
    );
    PlatformCapabilityReport::new(
        PlatformId::Linux,
        target.map(str::to_string),
        if cfg!(target_os = "linux") {
            SdkStatus::Present
        } else {
            SdkStatus::Missing
        },
        vec!["wgpu".to_string(), "headless".to_string()],
        vec![
            "gstreamer_profile".to_string(),
            "ffmpeg_profile".to_string(),
        ],
        vec!["pipewire".to_string(), "pulseaudio".to_string()],
        vec!["xdg_data".to_string(), "file_package".to_string()],
        vec![
            "keyboard".to_string(),
            "mouse".to_string(),
            "ime".to_string(),
            "gamepad".to_string(),
        ],
        vec![
            "window".to_string(),
            "resize".to_string(),
            "crash_bundle".to_string(),
        ],
        vec!["network_runtime_ai_profile_gated".to_string()],
    )
}
