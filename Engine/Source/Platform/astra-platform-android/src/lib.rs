use astra_platform::{
    build_fingerprint, PlatformCapabilityReport, PlatformId, SdkStatus, UnavailablePlatformFactory,
};

pub fn factory() -> UnavailablePlatformFactory {
    UnavailablePlatformFactory::new(PlatformId::Android)
}

pub fn probe(target: Option<&str>) -> PlatformCapabilityReport {
    PlatformCapabilityReport::unavailable(
        PlatformId::Android,
        target,
        if cfg!(target_os = "android")
            || std::env::var_os("ANDROID_HOME").is_some()
            || std::env::var_os("ANDROID_SDK_ROOT").is_some()
        {
            SdkStatus::Present
        } else {
            SdkStatus::Missing
        },
        build_fingerprint(
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            ["unavailable"],
        ),
    )
}
