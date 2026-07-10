use astra_platform::{
    build_fingerprint, PlatformCapabilityReport, PlatformId, SdkStatus, UnavailablePlatformFactory,
};

pub fn factory() -> UnavailablePlatformFactory {
    UnavailablePlatformFactory::new(PlatformId::Ios)
}

pub fn probe(target: Option<&str>) -> PlatformCapabilityReport {
    PlatformCapabilityReport::unavailable(
        PlatformId::Ios,
        target,
        if cfg!(target_os = "ios") || std::env::var_os("DEVELOPER_DIR").is_some() {
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
