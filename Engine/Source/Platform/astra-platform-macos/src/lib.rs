use astra_platform::{
    build_fingerprint, PlatformCapabilityReport, PlatformId, SdkStatus, UnavailablePlatformFactory,
};

pub fn factory() -> UnavailablePlatformFactory {
    UnavailablePlatformFactory::new(PlatformId::Macos)
}

pub fn probe(target: Option<&str>) -> PlatformCapabilityReport {
    PlatformCapabilityReport::unavailable(
        PlatformId::Macos,
        target,
        if cfg!(target_os = "macos") {
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
