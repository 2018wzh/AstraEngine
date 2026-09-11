mod audio_executor;
mod manager_app;
mod metadata_runtime;
mod platform_secret;
mod stage_renderer;
mod text_service;

use std::process::ExitCode;

fn main() -> ExitCode {
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
