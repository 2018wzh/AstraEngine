use astra_platform::{HostLaunchProfile, HostStartFuture, PlatformHostFactory};

#[cfg(not(target_os = "macos"))]
use astra_platform::{PlatformError, PlatformErrorCode, PlatformId};

#[cfg(target_os = "macos")]
pub use macos::MacosHostRunner;

#[cfg(target_os = "macos")]
mod macos;

mod macos_platform_factory;
pub use macos_platform_factory::factory;
#[cfg(all(target_os = "macos", feature = "platform-test-driver"))]
pub use macos_platform_factory::factory_with_test_roots;
#[cfg(target_os = "macos")]
pub use macos_platform_factory::main_thread_host;
#[cfg(target_os = "macos")]
use macos_platform_factory::HostRoots;
pub use macos_platform_factory::MacosPlatformFactory;
