mod diagnostics;
mod factory;
#[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
mod test_driver;

pub use diagnostics::*;
pub use factory::*;
#[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
pub use test_driver::*;

use astra_platform::{build_fingerprint, PlatformCapabilityReport, PlatformHostProfile};

#[cfg(not(target_os = "windows"))]
use astra_platform::{PlatformId, SdkStatus};

pub fn probe(target: Option<&str>) -> PlatformCapabilityReport {
    #[cfg(target_os = "windows")]
    {
        let profile = PlatformHostProfile::windows_release(
            target.unwrap_or("nativevn-game"),
            "com.astra.probe",
        );
        PlatformCapabilityReport::from_profile(
            &profile,
            build_fingerprint(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
                ["windows-host"],
            ),
            ["wgpu_hardware", "wmf", "wasapi", "saved_games"],
        )
        .expect("built-in Windows profile is valid")
    }
    #[cfg(not(target_os = "windows"))]
    {
        PlatformCapabilityReport::unavailable(
            PlatformId::Windows,
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
