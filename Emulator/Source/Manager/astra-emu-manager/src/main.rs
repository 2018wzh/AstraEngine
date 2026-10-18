mod manager_app;
mod metadata_runtime;
mod platform_secret;
mod stage_renderer;
#[cfg(test)]
#[path = "../../astra-emu-manager-core/src/test_native.rs"]
mod test_native;
mod text_service;

use clap::Parser;
use std::process::ExitCode;

// Native VM boot may hold large temporaries. Reserve once per session owner.
const FAMILY_OWNER_STACK_BYTES: usize = 8 * 1024 * 1024;

#[derive(Parser)]
struct Arguments {
    #[command(subcommand)]
    extension: Option<ExtensionCommand>,
}

#[derive(clap::Subcommand)]
pub(crate) enum ExtensionCommand {
    /// Install a trusted local Luau text extension, initially disabled.
    #[command(name = "extension-install")]
    Install {
        id: String,
        source: std::path::PathBuf,
    },
    #[command(name = "extension-enable")]
    Enable {
        id: String,
        #[arg(action = clap::ArgAction::Set)]
        enabled: bool,
    },
    #[command(name = "extension-configure")]
    Configure {
        id: String,
        configuration: std::path::PathBuf,
    },
    #[command(name = "extension-list")]
    List,
}

fn main() -> ExitCode {
    let arguments = Arguments::parse();
    if let Some(command) = arguments.extension {
        return match manager_app::run_extension_command(command) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        };
    }
    match manager_app::run_application() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            rfd::MessageDialog::new()
                .set_title("AstraEMU 启动失败")
                .set_description(error.to_string())
                .set_level(rfd::MessageLevel::Error)
                .set_buttons(rfd::MessageButtons::Ok)
                .show();
            ExitCode::FAILURE
        }
    }
}
