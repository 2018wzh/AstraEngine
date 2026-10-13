#[cfg(target_os = "linux")]
#[path = "package_completion.rs"]
mod package_completion;

#[cfg(target_os = "linux")]
#[path = "package_worker.rs"]
mod package_worker;

use astra_platform::{HostLaunchProfile, HostStartFuture, PlatformHostFactory};

#[cfg(not(target_os = "linux"))]
use astra_platform::{PlatformError, PlatformErrorCode, PlatformId};

#[cfg(target_os = "linux")]
mod linux;

mod linux_platform_factory;
pub use linux_platform_factory::factory;
#[cfg(all(target_os = "linux", feature = "platform-test-driver"))]
pub use linux_platform_factory::factory_with_test_roots;
#[cfg(target_os = "linux")]
use linux_platform_factory::HostRoots;
pub use linux_platform_factory::LinuxPlatformFactory;
