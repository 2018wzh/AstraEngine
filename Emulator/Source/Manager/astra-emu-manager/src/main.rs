mod audio_executor;
mod headless;
mod manager_app;
mod metadata_runtime;
mod platform_secret;
mod stage_renderer;
mod text_service;

use clap::Parser;
use std::process::ExitCode;

#[derive(Parser)]
struct Arguments {
    /// Run a local family startup test using the clocked null audio device.
    #[arg(long)]
    headless: Option<std::path::PathBuf>,
}

fn main() -> ExitCode {
    if let Some(configuration) = Arguments::parse().headless {
        return match headless::run(&configuration) {
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
