use astra_package::{PackageManifest, PackageReader};

use astra_platform::{
    migrate_host_profile_json, validate_host_profile, PlatformError, PlatformErrorCode,
    PlatformHostProfile,
};

use serde::{Deserialize, Serialize};

pub use astra_player_core::WEB_PLAYER_LIVE_EVIDENCE_SCHEMA;

#[cfg(any(target_arch = "wasm32", feature = "web-code-check"))]
mod browser;

mod web_player_live_evidence;
pub use web_player_live_evidence::validate_package;
pub use web_player_live_evidence::WebPlayerConfig;
pub use web_player_live_evidence::WebPlayerLiveEvidence;
