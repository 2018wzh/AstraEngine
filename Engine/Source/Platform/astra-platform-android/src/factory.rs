use astra_platform::{
    HostLaunchProfile, HostStartFuture, PlatformError, PlatformErrorCode, PlatformHostFactory,
    PlatformId,
};

#[derive(Debug, Clone, Default)]
pub struct AndroidPlatformFactory;

pub fn factory() -> AndroidPlatformFactory {
    AndroidPlatformFactory
}

impl PlatformHostFactory for AndroidPlatformFactory {
    fn start(&self, profile: HostLaunchProfile) -> HostStartFuture {
        Box::pin(async move {
            let platform = profile.require_platform()?;
            if platform.platform != PlatformId::Android {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidProfile,
                    "host.start",
                    "Android factory requires an Android profile",
                ));
            }
            astra_platform::validate_host_profile(platform)?;
            #[cfg(target_os = "android")]
            {
                crate::native::start_registered_activity(profile).await
            }
            #[cfg(not(target_os = "android"))]
            {
                Err(PlatformError::new(
                    PlatformErrorCode::UnsupportedPlatform,
                    "host.start",
                    "Android host requires the GameActivity native entrypoint",
                )
                .with_field("platform", PlatformId::Android.as_str()))
            }
        })
    }
}
