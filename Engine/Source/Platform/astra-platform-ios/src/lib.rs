use astra_platform::{PlatformCapabilityReport, PlatformId, ReportBackedPlatformHost, SdkStatus};

pub fn host(target: Option<&str>) -> ReportBackedPlatformHost {
    ReportBackedPlatformHost::new(probe(target))
}

pub fn probe(target: Option<&str>) -> PlatformCapabilityReport {
    tracing::info!(
        event = "platform.probe.start",
        platform = "ios",
        has_target = target.is_some(),
        "platform capability probe started"
    );
    PlatformCapabilityReport::new(
        PlatformId::Ios,
        target.map(str::to_string),
        if cfg!(target_os = "ios") || std::env::var_os("DEVELOPER_DIR").is_some() {
            SdkStatus::Present
        } else {
            SdkStatus::Missing
        },
        vec!["wgpu_metal".to_string()],
        vec!["avfoundation".to_string()],
        vec!["avaudio".to_string()],
        vec!["app_container".to_string(), "document_import".to_string()],
        vec![
            "touch".to_string(),
            "safe_area".to_string(),
            "gamepad".to_string(),
        ],
        vec![
            "foreground".to_string(),
            "background_resume".to_string(),
            "rotation".to_string(),
        ],
        vec![
            "network_runtime_ai_profile_gated".to_string(),
            "luau_no_jit".to_string(),
        ],
    )
}
