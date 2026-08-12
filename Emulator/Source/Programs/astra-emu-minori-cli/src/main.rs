use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::PathBuf,
    sync::Arc,
};

use astra_core::Hash256;
use astra_emu_family_support::mount_family_vfs;
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
    RecoverGarbroProfile {
        #[arg(long)]
        formats: PathBuf,
        #[arg(long)]
        title: String,
        #[arg(long)]
        game_dir: PathBuf,
        #[arg(long)]
        private_patch: PathBuf,
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
        /// Optional local-private path for the sanitized aggregate report.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    CensusMovies {
        #[arg(long)]
        game_dir: PathBuf,
        #[arg(long)]
        mount_profile: PathBuf,
        /// Optional local-private path for the sanitized aggregate report.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Scan every movie entry through bounded VFS ranges instead of only its prefix.
        #[arg(long)]
        full_scan: bool,
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
        Command::RecoverGarbroProfile { .. } => "recover_garbro_profile",
        Command::CensusScripts { .. } => "census_scripts",
        Command::CensusMedia { .. } => "census_media",
        Command::CensusMovies { .. } => "census_movies",
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
        Command::RecoverGarbroProfile {
            formats,
            title,
            game_dir,
            private_patch,
        } => importer::recover_profile(&formats, &title, &game_dir, &private_patch),
        Command::CensusScripts {
            game_dir,
            mount_profile,
        } => census(&game_dir, &mount_profile),
        Command::CensusMedia {
            game_dir,
            mount_profile,
            output,
        } => census_media(&game_dir, &mount_profile, output.as_deref()),
        Command::CensusMovies {
            game_dir,
            mount_profile,
            output,
            full_scan,
        } => census_movies(&game_dir, &mount_profile, output.as_deref(), full_scan),
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
    Ok(mount_family_vfs(
        "minori",
        game_dir,
        profile,
        vec![Arc::new(MinoriVfsFamilyFactory)],
    )?)
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
    let character_commands = census_character_commands(&scripts);
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "schema": "astra.emu.minori.sc_census.v4",
            "script_count": scripts.len(),
            "census": census,
            "audio_resources": audio_resources,
            "character_commands": character_commands,
        }))?
    );
    Ok(())
}

#[derive(Debug, Default, Serialize)]
struct CharacterCommandCensus {
    command_count: u64,
    unknown_mode_count: u64,
    unknown_extension_count: u64,
    modes: BTreeMap<String, CharacterModeCensus>,
}

#[derive(Debug, Default, Serialize)]
struct CharacterModeCensus {
    command_count: u64,
    arity_counts: BTreeMap<u32, u64>,
    numeric_bounds: BTreeMap<u32, NumericBounds>,
    extension_counts: BTreeMap<u32, BTreeMap<String, u64>>,
}

#[derive(Debug, Serialize)]
struct NumericBounds {
    minimum: i64,
    maximum: i64,
}

fn census_character_commands(scripts: &[ScScript]) -> CharacterCommandCensus {
    const VERIFIED_MODES: &[&str] = &[
        "allmove",
        "blendrate",
        "clear",
        "freeze",
        "keep",
        "load",
        "move",
        "moveofs",
        "off",
        "order",
        "order2",
        "pos",
        "seq",
        "size",
        "visible",
    ];
    let mut report = CharacterCommandCensus::default();
    for script in scripts {
        for line in &script.lines {
            let ScLineKind::Command { command } = &line.kind else {
                continue;
            };
            if command.opcode != "char" {
                continue;
            }
            report.command_count += 1;
            let Some(mode) = command.operands.first().and_then(operand_text) else {
                report.unknown_mode_count += 1;
                continue;
            };
            let mode = mode.to_ascii_lowercase();
            if !VERIFIED_MODES.contains(&mode.as_str()) {
                report.unknown_mode_count += 1;
                continue;
            }
            let mode_report = report.modes.entry(mode).or_default();
            mode_report.command_count += 1;
            *mode_report
                .arity_counts
                .entry(command.operands.len().saturating_sub(1) as u32)
                .or_default() += 1;
            for (position, operand) in command.operands.iter().skip(1).enumerate() {
                if let ScOperand::Integer { value } = operand {
                    let bounds = mode_report.numeric_bounds.entry(position as u32).or_insert(
                        NumericBounds {
                            minimum: *value,
                            maximum: *value,
                        },
                    );
                    bounds.minimum = bounds.minimum.min(*value);
                    bounds.maximum = bounds.maximum.max(*value);
                    continue;
                }
                let Some(value) = operand_text(operand) else {
                    continue;
                };
                let extension = std::path::Path::new(value)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .map(str::to_ascii_lowercase);
                let extension_is_valid = extension.as_ref().is_some_and(|extension| {
                    !extension.is_empty()
                        && extension.len() <= 8
                        && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
                });
                let Some(extension) = extension else {
                    continue;
                };
                if !extension_is_valid {
                    report.unknown_extension_count += 1;
                    continue;
                }
                *mode_report
                    .extension_counts
                    .entry(position as u32)
                    .or_default()
                    .entry(extension)
                    .or_default() += 1;
            }
        }
    }
    report
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
    output: Option<&std::path::Path>,
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
    let movie = collect_movie_container_census(&vfs, false)?;
    let report = serde_json::to_string(&serde_json::json!({
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
        "movie": movie,
    }))?;
    if let Some(output) = output {
        write_private_report(output, &report)?;
    } else {
        println!("{report}");
    }
    Ok(())
}

fn census_movies(
    game_dir: &std::path::Path,
    profile: &std::path::Path,
    output: Option<&std::path::Path>,
    full_scan: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let vfs = mount_minori(game_dir, profile)?;
    let report = serde_json::to_string(&serde_json::json!({
        "schema": "astra.emu.minori.movie_census.v1",
        "movie": collect_movie_container_census(&vfs, full_scan)?,
    }))?;
    if let Some(output) = output {
        write_private_report(output, &report)?;
    } else {
        println!("{report}");
    }
    Ok(())
}

fn collect_movie_container_census(
    vfs: &Arc<dyn astra_emu_family_core::LegacyMountedVfs>,
    full_scan: bool,
) -> Result<MovieContainerCensus, Box<dyn std::error::Error>> {
    let mut movie = MovieContainerCensus::default();
    for entry in vfs
        .manifest()
        .entries
        .iter()
        .filter(|entry| entry.source_id == "mov")
    {
        if entry.decoded_size == 0 {
            return Err("ASTRA_EMU_MINORI_MOVIE_EMPTY".into());
        }
        movie.entry_count = movie
            .entry_count
            .checked_add(1)
            .ok_or("ASTRA_EMU_MINORI_MOVIE_COUNT")?;
        movie.decoded_bytes = movie
            .decoded_bytes
            .checked_add(entry.decoded_size)
            .ok_or("ASTRA_EMU_MINORI_MOVIE_TOTAL_SIZE")?;
        let container = classify_movie_entry(vfs, entry, full_scan)?;
        *movie.containers.entry(container.into()).or_default() += 1;
    }
    Ok(movie)
}

fn classify_movie_entry(
    vfs: &Arc<dyn astra_emu_family_core::LegacyMountedVfs>,
    entry: &astra_emu_family_core::LegacyVfsEntry,
    full_scan: bool,
) -> Result<&'static str, Box<dyn std::error::Error>> {
    let scan_limit = if full_scan {
        entry.decoded_size
    } else {
        entry.decoded_size.min(MOVIE_PROBE_BYTES)
    };
    let mut offset = 0u64;
    let mut tail = Vec::new();
    while offset < scan_limit {
        let length = (scan_limit - offset).min(MOVIE_PROBE_BYTES);
        let bytes = vfs.read_range(&entry.uri, offset, length)?.bytes;
        if bytes.len() as u64 != length {
            return Err("ASTRA_EMU_MINORI_MOVIE_PROBE_SHORT".into());
        }
        let tail_length = tail.len();
        let mut probe = tail;
        probe.extend_from_slice(&bytes);
        if let Some((kind, found_offset)) = find_movie_container(&probe) {
            return Ok(classify_movie_container_at(
                kind,
                offset == 0 && found_offset == 0,
            ));
        }
        const MOVIE_SIGNATURE_TAIL_BYTES: usize = 11;
        let retain = probe.len().min(MOVIE_SIGNATURE_TAIL_BYTES);
        tail = probe[probe.len() - retain..].to_vec();
        offset = offset
            .checked_add(length)
            .ok_or("ASTRA_EMU_MINORI_MOVIE_SCAN_OFFSET")?;
        if tail_length > MOVIE_SIGNATURE_TAIL_BYTES {
            return Err("ASTRA_EMU_MINORI_MOVIE_SCAN_STATE".into());
        }
    }
    Ok("unrecognized")
}

fn write_private_report(
    path: &std::path::Path,
    report: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() {
        return Err("ASTRA_EMU_MINORI_REPORT_EXISTS".into());
    }
    let parent = path.parent().ok_or("ASTRA_EMU_MINORI_REPORT_PATH")?;
    if !parent.is_dir() {
        return Err("ASTRA_EMU_MINORI_REPORT_PARENT".into());
    }
    let file_name = path.file_name().ok_or("ASTRA_EMU_MINORI_REPORT_PATH")?;
    let temporary = parent.join(format!(".{}.tmp", file_name.to_string_lossy()));
    if temporary.exists() {
        return Err("ASTRA_EMU_MINORI_REPORT_TEMP_EXISTS".into());
    }
    std::fs::write(&temporary, report.as_bytes())?;
    std::fs::rename(&temporary, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })?;
    Ok(())
}

const MOVIE_PROBE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Default, Serialize)]
struct MovieContainerCensus {
    entry_count: u64,
    decoded_bytes: u64,
    containers: BTreeMap<String, u64>,
}

#[cfg(test)]
fn classify_movie_container(header: &[u8]) -> &'static str {
    let Some((kind, offset)) = find_movie_container(header) else {
        return "unrecognized";
    };
    classify_movie_container_at(kind, offset == 0)
}

fn classify_movie_container_at(kind: &'static str, at_start: bool) -> &'static str {
    if at_start {
        return kind;
    }
    match kind {
        "avi" => "avi_wrapped",
        "mpeg_program_stream" => "mpeg_program_stream_wrapped",
        "mpeg_video_es" => "mpeg_video_es_wrapped",
        "mpeg_pes" => "mpeg_pes_wrapped",
        "asf" => "asf_wrapped",
        "isobmff" => "isobmff_wrapped",
        "matroska" => "matroska_wrapped",
        "ogg" => "ogg_wrapped",
        _ => "unrecognized",
    }
}

fn find_movie_container(bytes: &[u8]) -> Option<(&'static str, usize)> {
    let mut pes_packet = None;
    for offset in 0..bytes.len() {
        let remaining = &bytes[offset..];
        if remaining.len() >= 12 && &remaining[..4] == b"RIFF" && &remaining[8..12] == b"AVI " {
            return Some(("avi", offset));
        }
        if remaining.starts_with(&[0, 0, 1]) && remaining.len() >= 4 {
            match remaining[3] {
                0xBA => return Some(("mpeg_program_stream", offset)),
                0xB3 => return Some(("mpeg_video_es", offset)),
                0xE0..=0xEF => {
                    pes_packet.get_or_insert(offset);
                }
                _ => {}
            }
        }
        if remaining.starts_with(&[0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11]) {
            return Some(("asf", offset));
        }
        if remaining.len() >= 8 && &remaining[4..8] == b"ftyp" {
            return Some(("isobmff", offset));
        }
        if remaining.starts_with(b"\x1A\x45\xDF\xA3") {
            return Some(("matroska", offset));
        }
        if remaining.starts_with(b"OggS") {
            return Some(("ogg", offset));
        }
    }
    pes_packet.map(|offset| ("mpeg_pes", offset))
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

#[cfg(test)]
mod tests {
    use super::{classify_movie_container, write_private_report};

    #[test]
    fn movie_container_inventory_classifies_only_explicit_signatures() {
        assert_eq!(classify_movie_container(b"RIFF\0\0\0\0AVI "), "avi");
        assert_eq!(
            classify_movie_container(&[0, 0, 1, 0xBA]),
            "mpeg_program_stream"
        );
        assert_eq!(
            classify_movie_container(&[0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11]),
            "asf"
        );
        assert_eq!(classify_movie_container(b"\0\0\0\0ftypisom"), "isobmff");
        assert_eq!(classify_movie_container(b"\x1A\x45\xDF\xA3"), "matroska");
        assert_eq!(
            classify_movie_container(b"opaqueRIFF\0\0\0\0AVI "),
            "avi_wrapped"
        );
        assert_eq!(
            classify_movie_container(&[
                0, 0, 1, 0xE0, 0x00, 0x00, 0x00, 0x00, 0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF,
                0x11,
            ]),
            "asf_wrapped"
        );
        assert_eq!(classify_movie_container(b"opaque"), "unrecognized");
    }

    #[test]
    fn private_report_write_is_atomic_and_refuses_overwrite() {
        let temporary = tempfile::tempdir().unwrap();
        let report = temporary.path().join("report.json");

        write_private_report(&report, "{\"schema\":\"test\"}").unwrap();
        assert_eq!(
            std::fs::read_to_string(&report).unwrap(),
            "{\"schema\":\"test\"}"
        );
        assert!(write_private_report(&report, "again").is_err());
        assert!(!temporary.path().join(".report.json.tmp").exists());
    }
}
