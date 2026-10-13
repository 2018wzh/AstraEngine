use super::*;

#[derive(Clone, Default)]
pub struct MacosPlatformFactory {
    #[cfg(target_os = "macos")]
    pub(super) roots: Option<HostRoots>,
    #[cfg(target_os = "macos")]
    pub(super) pending: std::sync::Arc<std::sync::Mutex<Option<macos::BackendSetup>>>,
    #[cfg(target_os = "macos")]
    pub(super) start_wake: Option<winit::event_loop::EventLoopProxy<()>>,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone)]
pub(super) struct HostRoots {
    pub(super) save_base: std::path::PathBuf,
    pub(super) bundle_root: std::path::PathBuf,
}

pub fn factory() -> MacosPlatformFactory {
    MacosPlatformFactory::default()
}

#[cfg(target_os = "macos")]
pub fn main_thread_host(
) -> Result<(MacosHostRunner, MacosPlatformFactory), astra_platform::PlatformError> {
    MacosHostRunner::new()
}

#[cfg(all(target_os = "macos", feature = "platform-test-driver"))]
pub fn factory_with_test_roots(
    save_base: impl AsRef<std::path::Path>,
    bundle_root: impl AsRef<std::path::Path>,
) -> MacosPlatformFactory {
    MacosPlatformFactory {
        roots: Some(HostRoots {
            save_base: save_base.as_ref().to_path_buf(),
            bundle_root: bundle_root.as_ref().to_path_buf(),
        }),
        pending: Default::default(),
        start_wake: None,
    }
}

impl PlatformHostFactory for MacosPlatformFactory {
    fn start(&self, profile: HostLaunchProfile) -> HostStartFuture {
        #[cfg(target_os = "macos")]
        {
            Box::pin(crate::factory::macos::start(
                profile,
                self.roots.clone(),
                self.pending.clone(),
                self.start_wake.clone(),
            ))
        }
        #[cfg(not(target_os = "macos"))]
        {
            Box::pin(async move {
                profile.require_platform()?;
                Err(PlatformError::new(
                    PlatformErrorCode::UnsupportedPlatform,
                    "host.start",
                    "macOS host can only start on macOS",
                )
                .with_field("platform", PlatformId::Macos.as_str()))
            })
        }
    }
}
