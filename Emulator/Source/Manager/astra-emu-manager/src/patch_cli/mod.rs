mod worker;

use astra_emu_patch::{list_recipes, PatchError, PatchErrorKind, PatchProgress, PatchRequest};
use clap::Subcommand;
use std::{
    error::Error,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Subcommand)]
pub(super) enum Command {
    /// List catalog records without opening or executing recipes.
    List {
        /// Catalog directory (defaults to metadata/patches/ beside this executable).
        #[arg(long, value_name = "DIR")]
        scripts: Option<PathBuf>,
    },
    /// Convert read-only inputs into a new independent output directory.
    Apply {
        #[arg(long, value_name = "FILE")]
        script: PathBuf,
        #[arg(long, value_name = "DIR")]
        source: PathBuf,
        /// Patch input directory (defaults to --source).
        #[arg(long, value_name = "DIR")]
        patch: Option<PathBuf>,
        #[arg(long, value_name = "NEW-DIR")]
        output: PathBuf,
    },
}

pub(super) fn run(command: Command) -> ExitCode {
    worker::install_panic_hook();
    match execute(command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report_error(&error, &mut io::stderr().lock());
            ExitCode::from(if error.kind() == PatchErrorKind::Cancelled {
                130
            } else {
                1
            })
        }
    }
}

fn execute(command: Command) -> Result<(), PatchError> {
    match command {
        Command::List { scripts } => {
            let scripts = match scripts {
                Some(scripts) => scripts,
                None => std::env::current_exe()
                    .map_err(|cause| {
                        PatchError::with_source(
                            PatchErrorKind::Io,
                            "cannot locate executable",
                            cause,
                        )
                    })?
                    .parent()
                    .ok_or_else(|| {
                        PatchError::new(PatchErrorKind::Path, "executable has no parent directory")
                    })?
                    .join("metadata")
                    .join("patches"),
            };
            let recipes = list_recipes(&scripts)?;
            let mut stdout = io::stdout().lock();
            // One JSON record per line preserves catalog order and escapes
            // tabs/newlines in user-authored titles without executing a recipe.
            for recipe in recipes {
                let record = serde_json::json!({
                    "id": recipe.id,
                    "title": recipe.title,
                    "script": recipe.script,
                });
                writeln!(stdout, "{record}")?;
            }
            Ok(())
        }
        Command::Apply {
            script,
            source,
            patch,
            output,
        } => {
            let cancelled = Arc::new(AtomicBool::new(false));
            let signal = cancelled.clone();
            ctrlc::set_handler(move || signal.store(true, Ordering::Release)).map_err(|cause| {
                PatchError::with_source(
                    PatchErrorKind::Io,
                    "cannot install cancellation handler",
                    cause,
                )
            })?;
            let request = PatchRequest {
                script,
                patch: patch.unwrap_or_else(|| source.clone()),
                source,
                output,
            };
            let result = worker::apply(request, cancelled, |progress| {
                report_progress(progress, &mut io::stderr().lock());
            })?;
            // The worker has already been joined. A late Ctrl-C must not turn
            // an already published successful output into cancellation.
            let mut stdout = io::stdout().lock();
            writeln!(stdout, "output: {}", result.output.display())?;
            writeln!(
                stdout,
                "receipt: {}",
                result.output.join("astra-patch-result.json").display()
            )?;
            Ok(())
        }
    }
}

fn report_progress(progress: PatchProgress, stderr: &mut impl Write) {
    // Recipe authors must supply phase labels without resource bodies or keys.
    if let Some(total) = progress.total {
        let _ = writeln!(
            stderr,
            "{}: {}/{}",
            progress.phase, progress.completed, total
        );
    } else {
        let _ = writeln!(stderr, "{}: {}", progress.phase, progress.completed);
    }
}

fn report_error(error: &PatchError, stderr: &mut impl Write) {
    let mut rendered = error.to_string();
    let _ = writeln!(stderr, "{rendered}");
    let mut source = error.source();
    while let Some(cause) = source {
        let message = cause.to_string();
        // Luau errors can already include their nested traceback/cause. Keep
        // standalone native causes while avoiding repeated embedded chains.
        if !message.is_empty() && !rendered.contains(&message) {
            let _ = writeln!(stderr, "caused by: {message}");
            rendered.push('\n');
            rendered.push_str(&message);
        }
        source = cause.source();
    }
}
