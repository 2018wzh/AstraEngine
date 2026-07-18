use std::{path::PathBuf, sync::Arc};

use astra_emu_family_support::LegacyVfsFamilyRegistry;
use astra_emu_minori::{parse_sc, MinoriVfsFamilyFactory, ScCensus, ScOpcodeCatalog};
use clap::{Parser, Subcommand};

mod garbro_nrbf;
mod importer;

#[derive(Debug, Parser)]
#[command(name = "astra-emu-minori-cli")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    ImportGarbroScheme {
        #[arg(long)]
        formats: PathBuf,
        #[arg(long)]
        title: String,
        #[arg(long)]
        game_dir: PathBuf,
    },
    CensusScripts {
        #[arg(long)]
        game_dir: PathBuf,
        #[arg(long)]
        mount_profile: PathBuf,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut observability = astra_observability::HostObservabilityConfig::for_cli("info");
    observability.role = astra_observability::HostRole::Cli;
    let _observability = astra_observability::init_host(observability)?;
    let command = Cli::parse().command;
    let action = match &command {
        Command::ImportGarbroScheme { .. } => "import_garbro_scheme",
        Command::CensusScripts { .. } => "census_scripts",
    };
    tracing::info!(event = "astra.emu.minori_cli.start", action);
    let result = match command {
        Command::ImportGarbroScheme {
            formats,
            title,
            game_dir,
        } => importer::import(&formats, &title, &game_dir),
        Command::CensusScripts {
            game_dir,
            mount_profile,
        } => census(&game_dir, &mount_profile),
    };
    if result.is_err() {
        tracing::error!(
            event = "astra.emu.minori_cli.failed",
            diagnostic_code = "ASTRA_EMU_MINORI_CLI_FAILED",
            action
        );
    } else {
        tracing::info!(event = "astra.emu.minori_cli.completed", action);
    }
    result
}

fn census(
    game_dir: &std::path::Path,
    profile: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut registry = LegacyVfsFamilyRegistry::default();
    registry.register(Arc::new(MinoriVfsFamilyFactory))?;
    let loaded = registry.load_profile(profile)?;
    let vfs = registry.mount("minori", game_dir, &loaded)?;
    let catalog = ScOpcodeCatalog::observed_minori();
    let mut scripts = Vec::new();
    for entry in vfs
        .manifest()
        .entries
        .iter()
        .filter(|entry| entry.media_kind == "script")
    {
        let bytes = vfs.read_range(&entry.uri, 0, entry.decoded_size)?.bytes;
        scripts.push(parse_sc(&bytes, &catalog)?);
    }
    let census = ScCensus::from_scripts(&scripts);
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "schema": "astra.emu.minori.sc_census.v1",
            "script_count": scripts.len(),
            "census": census,
        }))?
    );
    Ok(())
}
