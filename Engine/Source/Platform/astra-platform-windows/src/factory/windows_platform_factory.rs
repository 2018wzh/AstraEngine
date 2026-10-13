use super::*;

#[derive(Debug, Clone, Default)]
pub struct WindowsPlatformFactory {
    #[cfg(target_os = "windows")]
    pub(super) test_null_audio: bool,
    #[cfg(target_os = "windows")]
    pub(super) roots: Option<HostRoots>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsServiceFactory;

#[cfg(target_os = "windows")]
#[derive(Debug, Clone)]
pub(super) struct HostRoots {
    pub(super) save_base: std::path::PathBuf,
    pub(super) bundle_root: std::path::PathBuf,
}

pub fn factory() -> WindowsPlatformFactory {
    WindowsPlatformFactory::default()
}

pub fn service_factory() -> WindowsServiceFactory {
    WindowsServiceFactory
}

#[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
pub fn factory_with_test_roots(
    save_base: impl AsRef<std::path::Path>,
    bundle_root: impl AsRef<std::path::Path>,
) -> WindowsPlatformFactory {
    WindowsPlatformFactory {
        test_null_audio: false,
        roots: Some(HostRoots {
            save_base: save_base.as_ref().to_path_buf(),
            bundle_root: bundle_root.as_ref().to_path_buf(),
        }),
    }
}

#[cfg(target_os = "windows")]
impl WindowsPlatformFactory {
    /// Select a clocked silent output for this test process only.
    pub fn with_test_null_audio(mut self, enabled: bool) -> Self {
        self.test_null_audio = enabled;
        self
    }
}

impl PlatformHostFactory for WindowsPlatformFactory {
    fn start(&self, profile: HostLaunchProfile) -> HostStartFuture {
        #[cfg(target_os = "windows")]
        {
            Box::pin(crate::factory::windows::start(
                profile,
                self.roots.clone(),
                self.test_null_audio,
            ))
        }
        #[cfg(not(target_os = "windows"))]
        {
            Box::pin(async move {
                profile.require_platform()?;
                Err(PlatformError::new(
                    PlatformErrorCode::UnsupportedPlatform,
                    "host.start",
                    "Windows host can only start on Windows",
                )
                .with_field("platform", PlatformId::Windows.as_str()))
            })
        }
    }
}

impl PlatformHostFactory for WindowsServiceFactory {
    fn start(&self, profile: HostLaunchProfile) -> HostStartFuture {
        #[cfg(target_os = "windows")]
        {
            Box::pin(crate::factory::windows::start_services(profile))
        }
        #[cfg(not(target_os = "windows"))]
        {
            Box::pin(async move {
                profile.require_platform()?;
                Err(PlatformError::new(
                    PlatformErrorCode::UnsupportedPlatform,
                    "host.services.start",
                    "Windows services can only start on Windows",
                )
                .with_field("platform", PlatformId::Windows.as_str()))
            })
        }
    }
}
