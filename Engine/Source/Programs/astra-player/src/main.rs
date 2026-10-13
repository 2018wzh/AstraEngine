#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
mod bundle_config;

mod native_session_clock;

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
mod preview_control;

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
mod preview_transport;

use astra_observability::{init_host, ConsoleFormat, HostObservabilityConfig, HostRole};

use astra_player::{
    bundled_player_resource_root, load_bundled_observability, AndroidInputHost, LinuxUinputHost,
    MacosCgEventHost, PlayerObservabilityOverrides, WebCdpInputHost, WebLiveAutomationRequest,
    WindowsLiveAutomationRequest, WindowsSendInputHost,
};

use astra_player_core::{
    PlayerAutomationScript, PlayerHostCommandResult, PlayerInputTranscript, PlayerPlatform,
};

use std::{env, fs, path::PathBuf};

mod player_cli_error;
use player_cli_error::main;
use player_cli_error::PlayerCliError;
mod run_bundled_game;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use run_bundled_game::run_bundled_game;
mod ui_button_state;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use ui_button_state::execute_platform_load;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use ui_button_state::execute_platform_save;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use ui_button_state::gamepad_navigation;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use ui_button_state::log_consumed_vn_step;
use ui_button_state::parse_usize_arg;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use ui_button_state::player_platform_error;
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
use ui_button_state::run_bundled_game;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use ui_button_state::ui_button_state;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use ui_button_state::ui_pointer_button;
use ui_button_state::write_json;
