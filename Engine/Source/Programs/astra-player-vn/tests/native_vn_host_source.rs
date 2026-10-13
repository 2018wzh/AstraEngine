#[path = "support/native_package.rs"]
mod native_package;

use native_package::*;

use astra_core::Hash256;

use astra_media_core::SceneCommand;

use astra_package::PackageReader;

use astra_player_core::{PlayerHostCommand, PlayerHostResourceId};

use astra_player_vn::{
    NativeVnAudioOutput, NativeVnHostCommandSource, NativeVnRuntimeExecution,
    DEFAULT_NATIVE_VN_GPU_TEXTURE_CACHE_BYTES,
};

use astra_ui_core::{
    UiButtonState, UiInputEventKind, UiInsets, UiPoint, UiPointerButton, UiViewport, ValidateUi,
};

use astra_vn_core::{compile_astra_project, AstraSource, VnRunConfig};

use astra_vn_package::package_sections_for_project;

#[path = "support/glyph_lifecycle.rs"]
mod glyph_lifecycle;

#[path = "support/policy_loading.rs"]
mod policy_loading;

#[path = "native_vn_host_source/fixtures.rs"]
mod fixtures;
use fixtures::advance;
use fixtures::prepare_test_save_metadata;
use fixtures::product_package;
use fixtures::product_package_for;
use fixtures::source_for;
use fixtures::STORY;
#[path = "native_vn_host_source/launch_contract.rs"]
mod launch_contract;
#[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
use launch_contract::packaged_native_vn_scene_reaches_live_windows_wgpu_and_releases_resources;
#[path = "native_vn_host_source/input_contract.rs"]
mod input_contract;
#[path = "native_vn_host_source/packaged_player_accepts_explicit_product_provider_bindings.rs"]
mod packaged_player_accepts_explicit_product_provider_bindings;
