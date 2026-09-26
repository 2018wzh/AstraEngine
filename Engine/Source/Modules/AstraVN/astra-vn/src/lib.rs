//! Typed AstraVN product sessions and authoring entry points.

pub use astra_vn_core::{VnPlayerCommand, VnRunConfig, VnRuntime, VnWaitKind};
pub use astra_vn_presentation::{StageModel, VnPresentationProviderManifest};
pub use astra_vn_script::{
    compile_astra_project, format_astra_source, AstraSource, CompileAstraProjectOptions,
    FormatOptions, SystemStoryValidationStatus,
};
pub use astra_vn_system::{SystemStoryManifest, VnSystemUiProfileManifest};

mod limits;
pub use limits::{VnHostError, VnHostLimits};
mod session;
pub use session::{
    NativeVnStateView, NativeVnStepCommand, NativeVnStepInput, NativeVnStepOutput, VnSession,
    VnSessionConfig,
};
