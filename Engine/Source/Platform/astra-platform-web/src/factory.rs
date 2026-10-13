use astra_platform::{HostLaunchProfile, HostStartFuture, PlatformHostFactory};

#[cfg(not(target_arch = "wasm32"))]
use astra_platform::{PlatformError, PlatformErrorCode};

#[cfg(any(target_arch = "wasm32", feature = "web-code-check"))]
#[cfg_attr(
    all(feature = "web-code-check", not(target_arch = "wasm32")),
    allow(dead_code)
)]
mod browser;

mod web_platform_factory;
pub use web_platform_factory::factory;
pub use web_platform_factory::WebPlatformFactory;
