use astra_core::Hash256;

use astra_headless_protocol::{
    ButtonState, CanonicalAudioSnapshot, GamepadControl, Observation, PhysicalInput,
    ProductAdapterFactory, ProductFuture, ProductHostError, ProductOpenRequest,
    ProductPackageSource, ProductPerformanceObserver, ProductPerformanceSample, ProductSession,
    TouchPhase,
};

use astra_package::{
    AstraContainerReader, ContainerCryptoProvider, PackageReader, SourceUnlockPolicy,
};

use astra_platform::{
    SurfaceHandle, SurfaceRequest, WindowHandle, WindowRequest, HEADLESS_PRESENTATION_RATE_HZ,
};

use astra_player_core::{
    PlatformCommandSink, PlayerHostCommand, PlayerHostCommandBatch, PlayerHostCommandExecutor,
    PlayerHostCommandResult, PlayerHostResourceId,
};

use astra_ui_core::{
    UiButtonState, UiInputEventKind, UiNavigationAction, UiPoint, UiPointerButton, UiTouchPhase,
};

use astra_vn_core::VnRunConfig;

use std::{sync::Arc, time::Instant};

use astra_player_vn::{
    NativeVnDecodedCacheBudget, NativeVnHostCommandSource, NativeVnProductMediaHost,
    VnUiHostRequest,
};

#[cfg(test)]
mod cadence_tests {
    use super::presentation_substep_duration_ns;

    #[test]
    fn presentation_substeps_preserve_the_authoritative_tick_duration() {
        let tick = astra_headless_protocol::TICK_DURATION_NS;
        let first = presentation_substep_duration_ns(tick, 2, 0);
        let second = presentation_substep_duration_ns(tick, 2, 1);
        assert_eq!(first, 8_333_333);
        assert_eq!(second, 8_333_334);
        assert_eq!(first + second, tick);
    }
}

mod native_vn_product_adapter_factory;
use native_vn_product_adapter_factory::NativeVnHeadlessSession;
pub use native_vn_product_adapter_factory::NativeVnProductAdapterFactory;
mod native_vn_headless_session;

mod profile_started;

mod load;
use load::batch_presents_scene;
use load::binding;
use load::canonical_ms;
use load::normalized;
use load::presentation_substep_duration_ns;
use load::requires_active_input;
use load::ui_button_state;
use load::ui_navigation_action;
use load::ui_point;
use load::ui_pointer_button;
use load::ui_touch_phase;
