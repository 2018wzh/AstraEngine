use std::{path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand};
use serde::Serialize;
use tsuinosora_original_patcher::{
    apply, inspect_source, launch_windowed, verify, ApplyOptions, PatchError, WINDOW_LAUNCHER_NAME,
};

#[derive(Debug, Parser)]
#[command(name = "TsuiNoSoraOriginalPatcher")]
#[command(about = "Create and verify a separate patched copy of the 1999 Windows game")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Inspect {
        #[arg(long)]
        source: PathBuf,
    },
    Apply {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        projectorrays: Option<PathBuf>,
        #[arg(long)]
        locale_emulator: Option<PathBuf>,
    },
    Verify {
        #[arg(long)]
        output: PathBuf,
    },
}

#[derive(Serialize)]
struct Diagnostic<'a> {
    schema: &'static str,
    status: &'static str,
    code: &'a str,
    class: String,
    message: &'a str,
}

fn main() -> ExitCode {
    if is_installed_launcher() {
        return finish(installed_game_root().and_then(|root| launch_windowed(&root)));
    }
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let diagnostic = Diagnostic {
                schema: "tsuinosora.original_patch_diagnostic.v1",
                status: "blocked",
                code: error.code,
                class: error.class.to_string(),
                message: &error.message,
            };
            let encoded = serde_json::to_string(&diagnostic).unwrap_or_else(|_| {
                "{\"schema\":\"tsuinosora.original_patch_diagnostic.v1\",\"status\":\"blocked\",\"code\":\"TSUI_PATCH_DIAGNOSTIC_ENCODING_FAILED\"}".to_owned()
            });
            eprintln!("{encoded}");
            ExitCode::from(error.exit_code() as u8)
        }
    }
}

fn installed_game_root() -> Result<PathBuf, PatchError> {
    let executable = std::env::current_exe().map_err(|error| {
        PatchError::io(
            "TSUI_PATCH_LAUNCHER_PATH_UNAVAILABLE",
            "resolve installed launcher path",
            error,
        )
    })?;
    executable.parent().map(PathBuf::from).ok_or_else(|| {
        PatchError::validation(
            "TSUI_PATCH_LAUNCHER_LOCATION_INVALID",
            "installed launcher has no game directory",
        )
    })
}

fn is_installed_launcher() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.file_name().map(|name| name.to_owned()))
        .is_some_and(|name| {
            name.to_string_lossy()
                .eq_ignore_ascii_case(WINDOW_LAUNCHER_NAME)
        })
}

fn finish(result: Result<(), PatchError>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let diagnostic = Diagnostic {
                schema: "tsuinosora.original_patch_diagnostic.v1",
                status: "blocked",
                code: error.code,
                class: error.class.to_string(),
                message: &error.message,
            };
            eprintln!("{}", serde_json::to_string(&diagnostic).unwrap_or_default());
            ExitCode::from(error.exit_code() as u8)
        }
    }
}

fn run(cli: Cli) -> Result<(), PatchError> {
    match cli.command {
        Command::Inspect { source } => print_json(&inspect_source(&source)?),
        Command::Apply {
            source,
            output,
            projectorrays,
            locale_emulator,
        } => print_json(&apply(&ApplyOptions {
            source,
            output,
            projectorrays,
            locale_emulator,
        })?),
        Command::Verify { output } => print_json(&verify(&output)?),
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<(), PatchError> {
    let json = serde_json::to_string_pretty(value)?;
    println!("{json}");
    Ok(())
}
