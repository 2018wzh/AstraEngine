mod factory;
#[cfg(target_arch = "wasm32")]
mod services;

pub use factory::*;

use astra_platform::{build_fingerprint, PlatformCapabilityReport};

#[cfg(target_arch = "wasm32")]
use astra_platform::PlatformHostProfile;
#[cfg(not(target_arch = "wasm32"))]
use astra_platform::{PlatformId, SdkStatus};

pub fn probe(target: Option<&str>) -> PlatformCapabilityReport {
    #[cfg(target_arch = "wasm32")]
    {
        let profile =
            PlatformHostProfile::web_release(target.unwrap_or("nativevn-web"), "com.astra.probe");
        let mut report = PlatformCapabilityReport::from_profile(
            &profile,
            build_fingerprint(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
                ["wasm32", "webgpu"],
            ),
            std::iter::empty::<&str>(),
        )
        .expect("built-in Web profile is valid");
        report.diagnostics.push(astra_core::Diagnostic::blocking(
            "ASTRA_PLATFORM_RUNTIME_PROBE_REQUIRED",
            "provider availability requires a live browser conformance run",
        ));
        report
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        PlatformCapabilityReport::unavailable(
            PlatformId::Web,
            target,
            SdkStatus::Missing,
            build_fingerprint(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
                ["unavailable"],
            ),
        )
    }
}
