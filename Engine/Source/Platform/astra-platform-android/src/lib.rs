mod diagnostics;
mod factory;
mod lifecycle;
mod manifest;
mod policy;

#[cfg(target_os = "android")]
mod accessibility;
#[cfg(target_os = "android")]
mod audio;
#[cfg(target_os = "android")]
mod decode;

#[cfg(target_os = "android")]
mod native;

pub use diagnostics::*;
pub use factory::*;
pub use lifecycle::*;
pub use manifest::*;
pub use policy::*;

use astra_platform::{build_fingerprint, PlatformCapabilityReport, PlatformHostProfile};

pub fn probe(target: Option<&str>) -> PlatformCapabilityReport {
    let profile =
        PlatformHostProfile::android_release(target.unwrap_or("nativevn-game"), "com.astra.probe");
    let mut report = PlatformCapabilityReport::from_profile(
        &profile,
        build_fingerprint(
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            ["android-host", "vulkan", "mediacodec", "oboe"],
        ),
        std::iter::empty::<&str>(),
    )
    .expect("built-in Android profile is valid");
    report.diagnostics.push(astra_core::Diagnostic::blocking(
        "ASTRA_ANDROID_RUNTIME_PROBE_REQUIRED",
        "Android provider availability requires a live device conformance run",
    ));
    report
}

#[cfg(target_os = "android")]
pub use native::run_player_host;
