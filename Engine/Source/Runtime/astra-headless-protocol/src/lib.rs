mod artifact;
mod checkpoint;
mod framing;
mod input;
mod protocol;

pub use artifact::*;
pub use checkpoint::*;
pub use framing::*;
pub use input::*;
pub use protocol::*;

pub const USER_INPUT_SEQUENCE_SCHEMA: &str = "astra.user_input_sequence.v1";
pub const HEADLESS_PROTOCOL_SCHEMA: &str = "astra.headless_protocol.v1";
pub const HEADLESS_CHECKPOINT_CONFIG_SCHEMA: &str = "astra.headless_checkpoint_config.v2";
pub const HEADLESS_TOLERANCE_APPROVAL_SCHEMA: &str = "astra.headless_tolerance_approval.v2";
pub const HEADLESS_ARTIFACT_MANIFEST_SCHEMA: &str = "astra.headless_artifact_manifest.v2";
pub const HEADLESS_RUN_REPORT_SCHEMA: &str = "astra.headless_run_report.v2";
pub const HEADLESS_REVIEW_SCHEMA: &str = "astra.headless_review.v2";
pub const HEADLESS_REVIEW_BUNDLE_SCHEMA: &str = "astra.headless_review_bundle.v2";
pub const HEADLESS_PREFLIGHT_LINK_SCHEMA: &str = "astra.headless_preflight_link.v2";
pub const PLATFORM_RUN_IDENTITY_SCHEMA: &str = "astra.platform_run_identity.v1";
pub const TICK_DURATION_NS: u64 = 16_666_667;
pub const AUDIO_SAMPLE_RATE: u32 = 48_000;
pub const AUDIO_CHANNELS: u16 = 2;
pub const AUDIO_FRAMES_PER_TICK: u32 = 800;
