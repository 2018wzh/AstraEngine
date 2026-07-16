#[cfg(target_os = "macos")]
mod accessibility;
mod diagnostics;
mod factory;
mod media_performance;
#[cfg(all(target_os = "macos", feature = "platform-test-driver"))]
mod test_driver;

pub use diagnostics::*;
pub use factory::*;
pub use media_performance::*;
#[cfg(all(target_os = "macos", feature = "platform-test-driver"))]
pub use test_driver::*;

use astra_platform::{build_fingerprint, PlatformCapabilityReport};

#[cfg(target_os = "macos")]
use astra_platform::PlatformHostProfile;
#[cfg(not(target_os = "macos"))]
use astra_platform::{PlatformId, SdkStatus};

pub fn probe(target: Option<&str>) -> PlatformCapabilityReport {
    #[cfg(target_os = "macos")]
    {
        let profile = PlatformHostProfile::macos_release(
            target.unwrap_or("nativevn-game"),
            "com.astra.probe",
        );
        let mut report = PlatformCapabilityReport::from_profile(
            &profile,
            build_fingerprint(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
                ["macos-host", "appkit", "metal", "coreaudio", "avfoundation"],
            ),
            std::iter::empty::<&str>(),
        )
        .expect("built-in macOS profile is valid");
        report.diagnostics.push(astra_core::Diagnostic::blocking(
            "ASTRA_PLATFORM_RUNTIME_PROBE_REQUIRED",
            "provider availability requires a live host conformance run",
        ));
        report
    }
    #[cfg(not(target_os = "macos"))]
    {
        PlatformCapabilityReport::unavailable(
            PlatformId::Macos,
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
