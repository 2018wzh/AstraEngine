use std::{fs, path::PathBuf};

use astra_test::{ScenarioReport, ScenarioRunner};
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "astra")]
#[command(about = "AstraEngine Stage 1 command line")]
struct Cli {
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
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
            let scenario_report = ScenarioRunner::run_file(scenario)?;
            let encoded = encode_report(&scenario_report, format)?;
            if let Some(path) = report {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(path, &encoded)?;
            } else {
                println!("{encoded}");
            }
        }
        Command::Report {
            command: ReportCommand::Explain { report },
        } => {
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

fn encode_report(
    report: &ScenarioReport,
    format: ReportFormat,
) -> Result<String, Box<dyn std::error::Error>> {
    Ok(match format {
        ReportFormat::Json => serde_json::to_string_pretty(report)?,
        ReportFormat::Yaml => serde_yaml::to_string(report)?,
    })
}
