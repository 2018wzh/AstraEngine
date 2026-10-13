use super::*;

#[derive(Debug, Clone, Default)]
pub struct LinuxPlatformFactory {
    #[cfg(target_os = "linux")]
    pub(super) roots: Option<HostRoots>,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone)]
pub(super) struct HostRoots {
    pub(super) save_base: std::path::PathBuf,
    pub(super) bundle_root: std::path::PathBuf,
}

pub fn factory() -> LinuxPlatformFactory {
    LinuxPlatformFactory::default()
}

#[cfg(all(target_os = "linux", feature = "platform-test-driver"))]
pub fn factory_with_test_roots(
    save_base: impl AsRef<std::path::Path>,
    bundle_root: impl AsRef<std::path::Path>,
) -> LinuxPlatformFactory {
    LinuxPlatformFactory {
        roots: Some(HostRoots {
            save_base: save_base.as_ref().to_path_buf(),
            bundle_root: bundle_root.as_ref().to_path_buf(),
        }),
    }
}

impl PlatformHostFactory for LinuxPlatformFactory {
    fn start(&self, profile: HostLaunchProfile) -> HostStartFuture {
        #[cfg(target_os = "linux")]
        {
            Box::pin(crate::factory::linux::start(profile, self.roots.clone()))
        }
        #[cfg(not(target_os = "linux"))]
        {
            Box::pin(async move {
                profile.require_platform()?;
                Err(PlatformError::new(
                    PlatformErrorCode::UnsupportedPlatform,
                    "host.start",
                    "Linux host can only start on Linux",
                )
                .with_field("platform", PlatformId::Linux.as_str()))
            })
        }
    }
}
