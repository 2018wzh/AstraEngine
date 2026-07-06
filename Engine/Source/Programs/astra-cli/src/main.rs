use std::{env, fs, path::PathBuf};

use astra_test::{ScenarioReport, ScenarioRunner};
use clap::{Parser, Subcommand, ValueEnum};
use tracing::{debug, info};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt::writer::MakeWriterExt, EnvFilter};

type CliError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Parser)]
#[command(name = "astra")]
#[command(about = "AstraEngine Stage 1 command line")]
struct Cli {
    #[arg(long, global = true)]
    log_filter: Option<String>,
    #[arg(long, global = true, value_enum, default_value_t = LogFormat::Compact)]
    log_format: LogFormat,
    #[arg(long, global = true)]
    log_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Test {
        #[command(subcommand)]
        command: TestCommand,
    },
    Report {
        #[command(subcommand)]
        command: ReportCommand,
    },
}

#[derive(Subcommand)]
enum TestCommand {
    Run {
        scenario: PathBuf,
        #[arg(long)]
        headless: bool,
        #[arg(long)]
        report: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = ReportFormat::Yaml)]
        format: ReportFormat,
    },
}

#[derive(Subcommand)]
enum ReportCommand {
    Explain { report: PathBuf },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ReportFormat {
    Json,
    Yaml,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LogFormat {
    Compact,
    Json,
}

fn main() -> Result<(), CliError> {
    let cli = Cli::parse();
    let _log_guard = init_logging(&cli)?;
    match cli.command {
        Command::Test {
            command:
                TestCommand::Run {
                    scenario,
                    headless,
                    report,
                    format,
                },
        } => {
            if !headless {
                return Err("Stage 1 scenario runner requires --headless".into());
            }
            info!(
                headless,
                format = ?format,
                has_report_path = report.is_some(),
                "cli.test.run"
            );
            let scenario_report = ScenarioRunner::run_file(scenario)?;
            let encoded = encode_report(&scenario_report, format)?;
            if let Some(path) = report {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(path, &encoded)?;
                info!(
                    schema = %scenario_report.schema,
                    status = ?scenario_report.status,
                    "cli.report.write"
                );
            } else {
                debug!(
                    schema = %scenario_report.schema,
                    status = ?scenario_report.status,
                    "cli.report.stdout"
                );
                println!("{encoded}");
            }
        }
        Command::Report {
            command: ReportCommand::Explain { report },
        } => {
            info!("cli.report.explain");
            let text = fs::read_to_string(report)?;
            let report: ScenarioReport = if text.trim_start().starts_with('{') {
                serde_json::from_str(&text)?
            } else {
                serde_yaml::from_str(&text)?
            };
            println!("{}", report.explain());
        }
    }
    Ok(())
}

fn init_logging(cli: &Cli) -> Result<Option<WorkerGuard>, CliError> {
    let filter = cli
        .log_filter
        .clone()
        .or_else(|| env::var("ASTRA_LOG").ok())
        .unwrap_or_else(|| "info".to_string());
    let filter = EnvFilter::try_new(filter)?;

    if let Some(dir) = &cli.log_dir {
        fs::create_dir_all(dir)?;
        let file = tracing_appender::rolling::daily(dir, "astra.log");
        let (file, guard) = tracing_appender::non_blocking(file);
        let writer = std::io::stderr.and(file);
        match cli.log_format {
            LogFormat::Compact => tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(writer)
                .with_ansi(false)
                .compact()
                .try_init()?,
            LogFormat::Json => tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(writer)
                .with_ansi(false)
                .json()
                .try_init()?,
        }
        return Ok(Some(guard));
    }

    match cli.log_format {
        LogFormat::Compact => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .with_ansi(false)
            .compact()
            .try_init()?,
        LogFormat::Json => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .with_ansi(false)
            .json()
            .try_init()?,
    }
    Ok(None)
}

fn encode_report(report: &ScenarioReport, format: ReportFormat) -> Result<String, CliError> {
    Ok(match format {
        ReportFormat::Json => serde_json::to_string_pretty(report)?,
        ReportFormat::Yaml => serde_yaml::to_string(report)?,
    })
}
