use astra_core::Hash256;

use astra_headless_protocol::{
    ButtonState, InputMessage, JsonlWriter, ObservationPredicate, PhysicalInput,
    USER_INPUT_SEQUENCE_SCHEMA,
};

use astra_package::PackageReader;

use astra_platform::HeadlessHostProfile;

use astra_target::{TargetKind, TargetManifest};

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

#[path = "support/policy_actions.rs"]
mod policy_actions;

#[path = "target_platform/fixtures.rs"]
mod fixtures;
use fixtures::run_nativevn_headless;
#[path = "target_platform/desktop_bundle.rs"]
mod desktop_bundle;
#[path = "target_platform/web_bundle.rs"]
mod web_bundle;
use desktop_bundle::nativevn_minimal_story;
use desktop_bundle::run_astra;
use desktop_bundle::unique_case_dir;
use desktop_bundle::workspace_root;
use desktop_bundle::write_nativevn_minimal_fixture;
