use super::*;

#[derive(Debug, Clone, Copy, Default)]
pub struct WebPlatformFactory;

pub fn factory() -> WebPlatformFactory {
    WebPlatformFactory
}

impl PlatformHostFactory for WebPlatformFactory {
    fn start(&self, profile: HostLaunchProfile) -> HostStartFuture {
        #[cfg(target_arch = "wasm32")]
        {
            Box::pin(browser::start(profile))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Box::pin(async move {
                profile.require_platform()?;
                Err(PlatformError::new(
                    PlatformErrorCode::UnsupportedPlatform,
                    "host.start",
                    "Web host requires a wasm32 browser environment",
                ))
            })
        }
    }
}
