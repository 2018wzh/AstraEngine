use super::*;

thread_local! {
    static PLAYER: RefCell<Option<WebPlayerRuntime>> = const { RefCell::new(None) };
}

pub(super) struct WebPlayerRuntime {
    pub(super) _client: PlatformHostClient,
    pub(super) _window: WindowHandle,
    pub(super) _surface: SurfaceHandle,
    pub(super) _package_bytes: Vec<u8>,
}

#[derive(Clone)]
pub(super) struct WebEvidenceIdentity {
    pub(super) target: String,
    pub(super) profile: String,
    pub(super) package_hash: String,
}
