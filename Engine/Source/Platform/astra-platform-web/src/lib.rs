use astra_platform::{PlatformCapabilityReport, PlatformId, ReportBackedPlatformHost, SdkStatus};

pub fn host(target: Option<&str>) -> ReportBackedPlatformHost {
    ReportBackedPlatformHost::new(probe(target))
}

pub fn probe(target: Option<&str>) -> PlatformCapabilityReport {
    PlatformCapabilityReport::new(
        PlatformId::Web,
        target.map(str::to_string),
        if cfg!(target_arch = "wasm32") || std::env::var_os("ASTRA_WEB_SDK").is_some() {
            SdkStatus::Present
        } else {
            SdkStatus::Missing
        },
        vec!["webgpu".to_string(), "webgl_fallback".to_string()],
        vec!["webcodecs".to_string(), "software_profile".to_string()],
        vec!["webaudio".to_string()],
        vec![
            "opfs".to_string(),
            "indexeddb".to_string(),
            "file_api".to_string(),
            "http_range".to_string(),
        ],
        vec![
            "keyboard".to_string(),
            "mouse".to_string(),
            "touch".to_string(),
            "gamepad".to_string(),
        ],
        vec![
            "browser_launch".to_string(),
            "visibility_resume".to_string(),
            "worker".to_string(),
        ],
        vec![
            "browser_sandbox".to_string(),
            "network_runtime_ai_profile_gated".to_string(),
        ],
    )
}
