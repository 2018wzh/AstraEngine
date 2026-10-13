use astra_platform::{HostLaunchProfile, HostStartFuture, PlatformHostFactory};

#[cfg(not(target_os = "windows"))]
use astra_platform::{PlatformError, PlatformErrorCode, PlatformId};

#[cfg(target_os = "windows")]
mod windows;

mod windows_platform_factory;
pub use windows_platform_factory::factory;
#[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
pub use windows_platform_factory::factory_with_test_roots;
pub use windows_platform_factory::service_factory;
#[cfg(target_os = "windows")]
use windows_platform_factory::HostRoots;
pub use windows_platform_factory::WindowsPlatformFactory;
pub use windows_platform_factory::WindowsServiceFactory;
