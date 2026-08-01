use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::PathBuf,
    sync::Arc,
};

use astra_core::Hash256;
use astra_emu_family_support::LegacyVfsFamilyRegistry;
use astra_emu_minori::{
    parse_audio_resource_spec, parse_sc, MinoriAniArchive, MinoriSqzArchive,
    MinoriVfsFamilyFactory, ScCensus, ScLineKind, ScOpcodeCatalog, ScOperand, ScScript,
    MAX_ENTRY_BYTES,
};
use clap::{Parser, Subcommand};
use serde::Serialize;

mod garbro_nrbf;
mod importer;
mod inventory;

#[derive(Debug, Parser)]
#[command(name = "astra-emu-minori-cli")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    ScanArchives {
        #[arg(long)]
        game_dir: PathBuf,
    },
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
    CensusMedia {
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
        Command::ScanArchives { .. } => "scan_archives",
        Command::ImportGarbroScheme { .. } => "import_garbro_scheme",
        Command::CensusScripts { .. } => "census_scripts",
        Command::CensusMedia { .. } => "census_media",
    };
    tracing::info!(event = "astra.emu.minori_cli.start", action);
    let result = match command {
        Command::ScanArchives { game_dir } => {
            let report = inventory::scan_archive_inventory(&game_dir)?;
            println!("{}", serde_json::to_string(&report)?);
            Ok(())
        }
        Command::ImportGarbroScheme {
            formats,
            title,
            game_dir,
        } => importer::import(&formats, &title, &game_dir),
        Command::CensusScripts {
            game_dir,
            mount_profile,
        } => census(&game_dir, &mount_profile),
        Command::CensusMedia {
            game_dir,
            mount_profile,
        } => census_media(&game_dir, &mount_profile),
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

fn mount_minori(
    game_dir: &std::path::Path,
    profile: &std::path::Path,
) -> Result<Arc<dyn astra_emu_family_core::LegacyMountedVfs>, Box<dyn std::error::Error>> {
    let mut registry = LegacyVfsFamilyRegistry::default();
    registry.register(Arc::new(MinoriVfsFamilyFactory))?;
    let loaded = registry.load_profile(profile)?;
    Ok(registry.mount("minori", game_dir, &loaded)?)
}

fn census(
    game_dir: &std::path::Path,
    profile: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let vfs = mount_minori(game_dir, profile)?;
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
    let audio_resources = census_audio_resources(&scripts, vfs.manifest())?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "schema": "astra.emu.minori.sc_census.v3",
            "script_count": scripts.len(),
            "census": census,
            "audio_resources": audio_resources,
        }))?
    );
    Ok(())
}

#[derive(Debug, Default, Serialize)]
struct AudioResourceCensus {
    reference_count: u64,
    stop_token_count: u64,
    metadata_suffix_count: u64,
    malformed_count: u64,
    candidate_missing_count: u64,
    candidate_ambiguous_count: u64,
    reference_set_hash: Option<Hash256>,
    roles: BTreeMap<String, AudioResourceRoleCensus>,
}

#[derive(Debug, Default, Serialize)]
struct AudioResourceRoleCensus {
    reference_count: u64,
    exact_relative_count: u64,
    extension_appended_count: u64,
    ascii_casefold_count: u64,
    slash_normalized_count: u64,
    candidate_missing_count: u64,
    candidate_ambiguous_count: u64,
}

fn census_audio_resources(
    scripts: &[ScScript],
    manifest: &astra_emu_family_core::LegacyPackManifest,
) -> Result<AudioResourceCensus, Box<dyn std::error::Error>> {
    let mut entries = BTreeMap::<&str, Vec<&str>>::new();
    for entry in &manifest.entries {
        let prefix = format!("minori:/{}/", entry.source_id);
        let relative = entry
            .uri
            .strip_prefix(&prefix)
            .ok_or("ASTRA_EMU_MINORI_CENSUS_URI_ROLE")?;
        entries
            .entry(entry.source_id.as_str())
            .or_default()
            .push(relative);
    }
    let mut report = AudioResourceCensus::default();
    let mut identities = BTreeSet::<Vec<u8>>::new();
    for script in scripts {
        for line in &script.lines {
            let ScLineKind::Command { command } = &line.kind else {
                continue;
            };
            let role = match command.opcode.as_str() {
                "playbgm" => "bgm",
                "playse" | "playse2" | "playse3" => "se",
                "playvoice" => "voice",
                _ => continue,
            };
            report.reference_count += 1;
            let Some(token) = command.operands.first().and_then(operand_text) else {
                report.malformed_count += 1;
                continue;
            };
            let spec = match parse_audio_resource_spec(token) {
                Ok(spec) => spec,
                Err(_) => {
                    report.malformed_count += 1;
                    continue;
                }
            };
            if token.contains('[') {
                report.metadata_suffix_count += 1;
            }
            if spec.resource == "-" {
                report.stop_token_count += 1;
                continue;
            }
            let mut identity = role.as_bytes().to_vec();
            identity.push(0);
            identity.extend_from_slice(spec.resource.as_bytes());
            identities.insert(Hash256::from_sha256(&identity).as_bytes().to_vec());

            let role_report = report.roles.entry(role.into()).or_default();
            role_report.reference_count += 1;
            let available = entries.get(role).map(Vec::as_slice).unwrap_or_default();
            let normalized = spec.resource.replace('\\', "/");
            let mut matches = BTreeSet::new();
            let exact = available
                .iter()
                .copied()
                .filter(|entry| *entry == spec.resource)
                .collect::<Vec<_>>();
            if !exact.is_empty() {
                role_report.exact_relative_count += 1;
                matches.extend(exact);
            } else {
                let with_extension = format!("{}.ogg", spec.resource);
                let appended = available
                    .iter()
                    .copied()
                    .filter(|entry| *entry == with_extension)
                    .collect::<Vec<_>>();
                if !appended.is_empty() {
                    role_report.extension_appended_count += 1;
                    matches.extend(appended);
                } else {
                    let folded = available
                        .iter()
                        .copied()
                        .filter(|entry| {
                            entry.eq_ignore_ascii_case(&spec.resource)
                                || entry.eq_ignore_ascii_case(&with_extension)
                        })
                        .collect::<Vec<_>>();
                    if !folded.is_empty() {
                        role_report.ascii_casefold_count += 1;
                        matches.extend(folded);
                    } else if normalized != spec.resource {
                        let normalized_with_extension = format!("{normalized}.ogg");
                        let slash_matches = available
                            .iter()
                            .copied()
                            .filter(|entry| {
                                entry.eq_ignore_ascii_case(&normalized)
                                    || entry.eq_ignore_ascii_case(&normalized_with_extension)
                            })
                            .collect::<Vec<_>>();
                        if !slash_matches.is_empty() {
                            role_report.slash_normalized_count += 1;
                            matches.extend(slash_matches);
                        }
                    }
                }
            }
            match matches.len() {
                0 => {
                    role_report.candidate_missing_count += 1;
                    report.candidate_missing_count += 1;
                }
                1 => {}
                _ => {
                    role_report.candidate_ambiguous_count += 1;
                    report.candidate_ambiguous_count += 1;
                }
            }
        }
    }
    let identity_bytes = identities.into_iter().flatten().collect::<Vec<_>>();
    report.reference_set_hash = Some(Hash256::from_sha256(&identity_bytes));
    Ok(report)
}

fn operand_text(operand: &ScOperand) -> Option<&str> {
    match operand {
        ScOperand::Integer { .. } | ScOperand::Boolean { .. } => None,
        ScOperand::Operator { value } | ScOperand::Symbol { value } | ScOperand::Text { value } => {
            Some(value)
        }
    }
}

fn census_media(
    game_dir: &std::path::Path,
    profile: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let vfs = mount_minori(game_dir, profile)?;
    let mut png_entries = 0u64;
    let mut ani_entries = 0u64;
    let mut ani_frames = 0u64;
    let mut sqz_entries = 0u64;
    let mut sqz_frames = 0u64;
    let mut ogg_entries = 0u64;
    let mut database_entries = 0u64;
    let mut decoded_pixels = 0u64;
    let mut max_width = 0u32;
    let mut max_height = 0u32;
    let mut decoded_bytes = 0u64;
    for entry in vfs
        .manifest()
        .entries
        .iter()
        .filter(|entry| matches!(entry.source_id.as_str(), "bg" | "bgm"))
    {
        if entry.decoded_size > MAX_ENTRY_BYTES {
            return Err("ASTRA_EMU_MINORI_MEDIA_ENTRY_LIMIT".into());
        }
        let mut bytes = Vec::with_capacity(entry.decoded_size as usize);
        vfs.open_stream(&entry.uri)?
            .take(MAX_ENTRY_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 != entry.decoded_size {
            return Err("ASTRA_EMU_MINORI_MEDIA_ENTRY_SIZE".into());
        }
        decoded_bytes = decoded_bytes
            .checked_add(entry.decoded_size)
            .ok_or("ASTRA_EMU_MINORI_MEDIA_TOTAL_SIZE")?;
        let extension = entry
            .uri
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .unwrap_or_default();
        match extension.as_str() {
            "png" => {
                let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
                    .map_err(|_| "ASTRA_EMU_MINORI_MEDIA_PNG")?;
                record_dimensions(
                    image.width(),
                    image.height(),
                    &mut decoded_pixels,
                    &mut max_width,
                    &mut max_height,
                )?;
                png_entries += 1;
            }
            "ani" => {
                let archive = MinoriAniArchive::parse(Arc::<[u8]>::from(bytes))?;
                for index in 0..archive.frames().len() {
                    let frame = archive.decode_frame(index)?;
                    record_dimensions(
                        frame.width(),
                        frame.height(),
                        &mut decoded_pixels,
                        &mut max_width,
                        &mut max_height,
                    )?;
                    ani_frames += 1;
                }
                ani_entries += 1;
            }
            "sqz" => {
                let archive = MinoriSqzArchive::parse(Arc::<[u8]>::from(bytes))?;
                for index in 0..archive.frames().len() {
                    let frame = archive.decode_frame(index)?;
                    record_dimensions(
                        frame.width(),
                        frame.height(),
                        &mut decoded_pixels,
                        &mut max_width,
                        &mut max_height,
                    )?;
                    sqz_frames += 1;
                }
                sqz_entries += 1;
            }
            "ogg" if bytes.starts_with(b"OggS") => ogg_entries += 1,
            "db" => database_entries += 1,
            _ => return Err("ASTRA_EMU_MINORI_MEDIA_FORMAT_UNKNOWN".into()),
        }
    }
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "schema": "astra.emu.minori.media_census.v1",
            "entry_count": png_entries + ani_entries + sqz_entries + ogg_entries + database_entries,
            "decoded_bytes": decoded_bytes,
            "png_entries": png_entries,
            "ani_entries": ani_entries,
            "ani_frames": ani_frames,
            "sqz_entries": sqz_entries,
            "sqz_frames": sqz_frames,
            "ogg_entries": ogg_entries,
            "database_entries": database_entries,
            "decoded_pixels": decoded_pixels,
            "max_width": max_width,
            "max_height": max_height,
        }))?
    );
    Ok(())
}

fn record_dimensions(
    width: u32,
    height: u32,
    decoded_pixels: &mut u64,
    max_width: &mut u32,
    max_height: &mut u32,
) -> Result<(), Box<dyn std::error::Error>> {
    *decoded_pixels = decoded_pixels
        .checked_add(u64::from(width) * u64::from(height))
        .ok_or("ASTRA_EMU_MINORI_MEDIA_PIXEL_TOTAL")?;
    *max_width = (*max_width).max(width);
    *max_height = (*max_height).max(height);
    Ok(())
}
