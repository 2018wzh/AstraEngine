mod factory;

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
        PlatformCapabilityReport::from_profile(
            &profile,
            build_fingerprint(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
                ["wasm32", "webgpu"],
            ),
            ["webgpu", "webcodecs", "webaudio", "opfs"],
        )
        .expect("built-in Web profile is valid")
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
