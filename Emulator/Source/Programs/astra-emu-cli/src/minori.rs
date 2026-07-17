use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use astra_emu_family_api::LegacyMountedVfs;
use astra_emu_manager_core::{DecoderLimits, TrustedDecoderSession, DECODER_CAPABILITY};
use astra_emu_minori::{
    parse_sc, MinoriMountedVfs, PazArchiveConfig, PlaintextCache, ScCensus, ScOpcodeCatalog,
    DEFAULT_CACHE_ENTRY_LIMIT_BYTES, DEFAULT_CACHE_LIMIT_BYTES, REQUIRED_ARCHIVE_ROLES,
};
use clap::Subcommand;
use flate2::read::ZlibDecoder;
use globset::Glob;

#[derive(Debug, Subcommand)]
pub enum MinoriCommand {
    /// Mount a foreground, read-only FUSE view on Linux.
    Mount {
        #[arg(long)]
        game_dir: PathBuf,
        #[arg(long)]
        mountpoint: PathBuf,
        #[arg(long)]
        patch: Option<PathBuf>,
        #[arg(long, default_value_t = 2)]
        version: u8,
        #[arg(long, value_parser = parse_u32, default_value = "0")]
        index_size_xor: u32,
        #[arg(long, default_value = "minori.paz")]
        decoder: String,
    },
    /// Verify the six required PAZ archives, their indices, entries, and trusted decoder.
    Verify {
        #[arg(long)]
        game_dir: PathBuf,
        #[arg(long)]
        patch: Option<PathBuf>,
        #[arg(long, default_value_t = 2)]
        version: u8,
        #[arg(long, value_parser = parse_u32, default_value = "0")]
        index_size_xor: u32,
        #[arg(long, default_value = "minori.paz")]
        decoder: String,
    },
    /// Run a payload-free command and operand census over every script entry.
    CensusScripts {
        #[arg(long)]
        game_dir: PathBuf,
        #[arg(long)]
        patch: Option<PathBuf>,
        #[arg(long, default_value_t = 2)]
        version: u8,
        #[arg(long, value_parser = parse_u32, default_value = "0")]
        index_size_xor: u32,
        #[arg(long, default_value = "minori.paz")]
        decoder: String,
    },
    /// Extract selected decoded entries after a complete destination preflight.
    Extract {
        #[arg(long)]
        game_dir: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        patch: Option<PathBuf>,
        #[arg(long, default_value_t = 2)]
        version: u8,
        #[arg(long, value_parser = parse_u32, default_value = "0")]
        index_size_xor: u32,
        #[arg(long, default_value = "minori.paz")]
        decoder: String,
        #[arg(long)]
        role: Option<String>,
        #[arg(long)]
        glob: Option<String>,
        #[arg(long)]
        entry: Option<String>,
    },
    /// Import one title's validated Musica/PAZ scheme into a private Luau patch.
    ImportGarbroScheme {
        #[arg(long)]
        formats: PathBuf,
        #[arg(long)]
        title: String,
        #[arg(long)]
        game_dir: PathBuf,
    },
}

pub fn run(command: MinoriCommand) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        MinoriCommand::Mount {
            game_dir,
            mountpoint,
            patch,
            version,
            index_size_xor,
            decoder,
        } => {
            let vfs = build_vfs(
                &game_dir,
                patch.as_deref(),
                version,
                index_size_xor,
                &decoder,
            )?;
            #[cfg(target_os = "linux")]
            super::minori_fuse::mount(vfs, mountpoint)?;
            #[cfg(not(target_os = "linux"))]
            {
                let _ = (vfs, mountpoint);
                return Err("ASTRA_EMU_MINORI_FUSE_UNSUPPORTED".into());
            }
        }
        MinoriCommand::Verify {
            game_dir,
            patch,
            version,
            index_size_xor,
            decoder,
        } => {
            let vfs = build_vfs(
                &game_dir,
                patch.as_deref(),
                version,
                index_size_xor,
                &decoder,
            )?;
            let mut cache_hits = 0usize;
            let mut cache_misses = 0usize;
            for entry in &vfs.manifest().entries {
                let prefix_len = entry.size.min(4096);
                let result = vfs.read_range(&entry.uri, 0, prefix_len)?;
                if result.bytes.len() as u64 != prefix_len {
                    return Err("ASTRA_EMU_MINORI_VERIFY_SHORT_READ".into());
                }
                if result.cache_hit {
                    cache_hits += 1;
                } else {
                    cache_misses += 1;
                }

                let tail_len = entry.size.min(4096);
                let tail_offset = entry.size.saturating_sub(tail_len);
                let tail = vfs.read_range(&entry.uri, tail_offset, tail_len)?;
                if tail.bytes.len() as u64 != tail_len {
                    return Err("ASTRA_EMU_MINORI_VERIFY_RANDOM_SHORT_READ".into());
                }
            }
            println!(
                "{{\"schema\":\"astra.emu.minori.verify.v1\",\"status\":\"passed\",\"archive_count\":6,\"entry_count\":{},\"cache_hits\":{},\"cache_misses\":{},\"random_range_reads\":{}}}",
                vfs.manifest().entries.len(),
                cache_hits,
                cache_misses,
                vfs.manifest().entries.len()
            );
        }
        MinoriCommand::Extract {
            game_dir,
            output,
            patch,
            version,
            index_size_xor,
            decoder,
            role,
            glob,
            entry,
        } => {
            let vfs = build_vfs(
                &game_dir,
                patch.as_deref(),
                version,
                index_size_xor,
                &decoder,
            )?;
            extract(
                vfs.as_ref(),
                &output,
                role.as_deref(),
                glob.as_deref(),
                entry.as_deref(),
            )?;
            println!("{{\"schema\":\"astra.emu.minori.extract.v1\",\"status\":\"passed\"}}");
        }
        MinoriCommand::CensusScripts {
            game_dir,
            patch,
            version,
            index_size_xor,
            decoder,
        } => {
            let vfs = build_vfs(
                &game_dir,
                patch.as_deref(),
                version,
                index_size_xor,
                &decoder,
            )?;
            let catalog = ScOpcodeCatalog::observed_minori();
            let mut scripts = Vec::new();
            for entry in vfs.manifest().entries.iter().filter(|entry| {
                entry.uri.starts_with("minori:/scr/")
                    && entry.uri.to_ascii_lowercase().ends_with(".sc")
            }) {
                let read = vfs.read_range(&entry.uri, 0, entry.size)?;
                if read.bytes.len() as u64 != entry.size {
                    return Err("ASTRA_EMU_MINORI_CENSUS_SHORT_READ".into());
                }
                scripts.push(parse_sc(&read.bytes, &catalog)?);
            }
            if scripts.is_empty() {
                return Err("ASTRA_EMU_MINORI_CENSUS_EMPTY".into());
            }
            let census = ScCensus::from_scripts(&scripts);
            let report = serde_json::json!({
                "schema": "astra.emu.minori.sc_census.v1",
                "status": "passed",
                "census": census,
            });
            println!("{}", serde_json::to_string(&report)?);
        }
        MinoriCommand::ImportGarbroScheme {
            formats,
            title,
            game_dir,
        } => import_garbro_scheme(&formats, &title, &game_dir)?,
    }
    Ok(())
}

fn build_vfs(
    game_dir: &Path,
    patch: Option<&Path>,
    version: u8,
    index_size_xor: u32,
    decoder_id: &str,
) -> Result<Arc<MinoriMountedVfs>, Box<dyn std::error::Error>> {
    for role in REQUIRED_ARCHIVE_ROLES {
        let path = game_dir.join(format!("{role}.paz"));
        let metadata = fs::metadata(path).map_err(|_| "ASTRA_EMU_MINORI_ARCHIVE_MISSING")?;
        if metadata.len() == 0 {
            return Err("ASTRA_EMU_MINORI_ARCHIVE_EMPTY".into());
        }
    }
    let patch = patch
        .map(PathBuf::from)
        .unwrap_or_else(|| game_dir.join("astraemu.patch.luau"));
    let source = fs::read_to_string(&patch).map_err(|_| "ASTRA_EMU_DECODER_PATCH_MISSING")?;
    let capabilities = BTreeSet::from([DECODER_CAPABILITY.to_owned()]);
    let session = Arc::new(TrustedDecoderSession::load(
        &source,
        &capabilities,
        DecoderLimits::default(),
    )?);
    let decoder = Arc::new(session.decoder(decoder_id)?);
    let configs = REQUIRED_ARCHIVE_ROLES
        .iter()
        .map(|role| PazArchiveConfig {
            role: (*role).into(),
            path: game_dir.join(format!("{role}.paz")),
            version,
            index_size_xor,
        })
        .collect();
    let cache_root = directories::ProjectDirs::from("org", "AstraEngine", "AstraEMU")
        .ok_or("ASTRA_EMU_MINORI_CACHE_DIRECTORY")?
        .cache_dir()
        .join("minori");
    let cache = PlaintextCache::new(
        cache_root,
        DEFAULT_CACHE_LIMIT_BYTES,
        DEFAULT_CACHE_ENTRY_LIMIT_BYTES,
    )?;
    Ok(Arc::new(MinoriMountedVfs::mount_with_cache(
        "minori-main",
        "minori:/",
        configs,
        decoder,
        Some(cache),
    )?))
}

fn extract(
    vfs: &dyn LegacyMountedVfs,
    output: &Path,
    role: Option<&str>,
    pattern: Option<&str>,
    single: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let selectors = [role.is_some(), pattern.is_some(), single.is_some()]
        .into_iter()
        .filter(|value| *value)
        .count();
    if selectors > 1 {
        return Err("ASTRA_EMU_MINORI_EXTRACT_SELECTOR_CONFLICT".into());
    }
    if let Some(role) = role {
        if !REQUIRED_ARCHIVE_ROLES.contains(&role) {
            return Err("ASTRA_EMU_MINORI_EXTRACT_ROLE".into());
        }
    }
    let matcher = match pattern.map(Glob::new).transpose() {
        Ok(value) => value.map(|glob| glob.compile_matcher()),
        Err(_) => return Err("ASTRA_EMU_MINORI_EXTRACT_GLOB".into()),
    };
    let selected = vfs
        .manifest()
        .entries
        .iter()
        .filter(|item| {
            role.is_none_or(|value| item.uri.starts_with(&format!("minori:/{value}/")))
                && single.is_none_or(|value| item.uri == value)
                && matcher
                    .as_ref()
                    .is_none_or(|value| value.is_match(&item.uri))
        })
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err("ASTRA_EMU_MINORI_EXTRACT_EMPTY".into());
    }
    if output.exists() {
        return Err("ASTRA_EMU_MINORI_EXTRACT_OUTPUT_EXISTS".into());
    }
    let output_parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_parent)?;
    let output_name = output
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or("ASTRA_EMU_MINORI_EXTRACT_OUTPUT")?;
    let staging = output_parent.join(format!(".{output_name}.astra-extract-tmp"));
    if staging.exists() {
        return Err("ASTRA_EMU_MINORI_EXTRACT_STAGING_EXISTS".into());
    }
    let mut folded = BTreeSet::new();
    let mut required = 0u64;
    let mut relative_paths = Vec::new();
    for item in &selected {
        required = required
            .checked_add(item.size)
            .ok_or("ASTRA_EMU_MINORI_EXTRACT_SIZE_OVERFLOW")?;
        let relative = item
            .uri
            .strip_prefix("minori:/")
            .ok_or("ASTRA_EMU_MINORI_EXTRACT_URI")?;
        let folded_path = relative.to_lowercase();
        if !folded.insert(folded_path) {
            return Err("ASTRA_EMU_MINORI_EXTRACT_CASE_COLLISION".into());
        }
        relative_paths.push(PathBuf::from(relative));
    }
    if fs2::available_space(output_parent)? < required {
        return Err("ASTRA_EMU_MINORI_EXTRACT_CAPACITY".into());
    }
    fs::create_dir(&staging)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staging, fs::Permissions::from_mode(0o700))?;
    }
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        for (item, relative) in selected.iter().zip(relative_paths.iter()) {
            let destination = staging.join(relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            let temporary = destination.with_extension(format!(
                "{}.astra-tmp",
                destination
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
            ));
            let mut options = OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut target = options.open(&temporary)?;
            let mut stream = vfs.open_stream(&item.uri)?;
            std::io::copy(&mut stream, &mut target)?;
            target.sync_all()?;
            drop(target);
            fs::rename(&temporary, &destination)?;
        }
        fs::rename(&staging, output)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn import_garbro_scheme(
    formats: &Path,
    title: &str,
    game_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if title.is_empty() || title.len() > 512 {
        return Err("ASTRA_EMU_GARBRO_TITLE".into());
    }
    if !game_dir.is_dir() {
        return Err("ASTRA_EMU_GARBRO_GAME_DIRECTORY".into());
    }
    let bytes = fs::read(formats)?;
    if bytes.len() < 12 || &bytes[..8] != b"GARbroDB" {
        return Err("ASTRA_EMU_GARBRO_HEADER".into());
    }
    let mut decoded = Vec::new();
    ZlibDecoder::new(&bytes[12..])
        .take(256 * 1024 * 1024)
        .read_to_end(&mut decoded)?;
    if decoded.len() >= 256 * 1024 * 1024 {
        return Err("ASTRA_EMU_GARBRO_SIZE".into());
    }
    let message = nrbf::RemotingMessage::parse(&decoded).map_err(|_| "ASTRA_EMU_GARBRO_NRBF")?;
    let root = match &message {
        nrbf::RemotingMessage::Value(value) => value,
        _ => return Err("ASTRA_EMU_GARBRO_ROOT".into()),
    };
    let records = find_dictionary_values(root, title)?;
    let record = match records.as_slice() {
        [record] => *record,
        [] => return Err("ASTRA_EMU_GARBRO_TITLE_NOT_FOUND".into()),
        _ => return Err("ASTRA_EMU_GARBRO_TITLE_DUPLICATE".into()),
    };
    let roles = extract_roles(record)?;
    let patch_path = game_dir.join("astraemu.patch.luau");
    if patch_path.exists() {
        return Err("ASTRA_EMU_GARBRO_PATCH_EXISTS".into());
    }
    let temporary = game_dir.join(".astraemu.patch.luau.tmp");
    let patch = render_patch(&roles)?;
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(patch.as_bytes())?;
    file.sync_all()?;
    drop(file);
    if let Err(error) = fs::rename(&temporary, &patch_path) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    println!("{{\"schema\":\"astra.emu.minori.garbro_import.v1\",\"status\":\"passed\"}}");
    Ok(())
}

#[derive(Debug)]
struct ImportedRole {
    index_key: Vec<u8>,
    data_key: Vec<u8>,
    type_keys: BTreeMap<String, String>,
    version: i32,
}

const MAX_GARBRO_GRAPH_NODES: usize = 1_000_000;
const MAX_GARBRO_GRAPH_DEPTH: usize = 128;
const MAX_GARBRO_DICTIONARY_ENTRIES: usize = 100_000;

fn find_dictionary_values<'a>(
    value: &'a nrbf::Value<'a>,
    key: &str,
) -> Result<Vec<&'a nrbf::Value<'a>>, Box<dyn std::error::Error>> {
    let mut stack = vec![(value, 0usize)];
    let mut visited = 0usize;
    let mut matches = Vec::new();
    while let Some((value, depth)) = stack.pop() {
        visited += 1;
        if visited > MAX_GARBRO_GRAPH_NODES || depth > MAX_GARBRO_GRAPH_DEPTH {
            return Err("ASTRA_EMU_GARBRO_GRAPH_LIMIT".into());
        }
        match value {
            nrbf::Value::Object(object) => {
                let pair_key = object
                    .members
                    .get("key")
                    .or_else(|| object.members.get("Key"));
                if matches!(pair_key, Some(nrbf::Value::String(value)) if *value == key) {
                    let item = object
                        .members
                        .get("value")
                        .or_else(|| object.members.get("Value"))
                        .ok_or("ASTRA_EMU_GARBRO_DICTIONARY_PAIR")?;
                    matches.push(item);
                    if matches.len() > 1 {
                        return Ok(matches);
                    }
                }
                stack.extend(object.members.values().map(|value| (value, depth + 1)));
            }
            nrbf::Value::Array(values) => {
                stack.extend(values.iter().map(|value| (value, depth + 1)));
            }
            _ => {}
        }
    }
    Ok(matches)
}

fn dictionary_entries<'a>(
    value: &'a nrbf::Value<'a>,
) -> Result<Vec<(&'a str, &'a nrbf::Value<'a>)>, Box<dyn std::error::Error>> {
    let mut stack = vec![(value, 0usize)];
    let mut visited = 0usize;
    let mut output = Vec::new();
    let mut keys = BTreeSet::new();
    while let Some((value, depth)) = stack.pop() {
        visited += 1;
        if visited > MAX_GARBRO_GRAPH_NODES || depth > MAX_GARBRO_GRAPH_DEPTH {
            return Err("ASTRA_EMU_GARBRO_GRAPH_LIMIT".into());
        }
        match value {
            nrbf::Value::Object(object) => {
                let key = object
                    .members
                    .get("key")
                    .or_else(|| object.members.get("Key"));
                let item = object
                    .members
                    .get("value")
                    .or_else(|| object.members.get("Value"));
                if let (Some(nrbf::Value::String(key)), Some(item)) = (key, item) {
                    let folded = key.to_lowercase();
                    if !keys.insert(folded) || output.len() == MAX_GARBRO_DICTIONARY_ENTRIES {
                        return Err("ASTRA_EMU_GARBRO_DICTIONARY_DUPLICATE".into());
                    }
                    output.push((*key, item));
                }
                stack.extend(object.members.values().map(|value| (value, depth + 1)));
            }
            nrbf::Value::Array(values) => {
                stack.extend(values.iter().map(|value| (value, depth + 1)));
            }
            _ => {}
        }
    }
    Ok(output)
}

fn object_member<'a>(value: &'a nrbf::Value<'a>, names: &[&str]) -> Option<&'a nrbf::Value<'a>> {
    let nrbf::Value::Object(object) = value else {
        return None;
    };
    names.iter().find_map(|name| object.members.get(*name))
}

fn extract_roles(
    record: &nrbf::Value<'_>,
) -> Result<BTreeMap<String, ImportedRole>, Box<dyn std::error::Error>> {
    let class = match record {
        nrbf::Value::Object(object) => object.class,
        _ => return Err("ASTRA_EMU_GARBRO_OBJECT".into()),
    };
    if !class.ends_with("PazScheme") {
        return Err("ASTRA_EMU_GARBRO_SCHEME_TYPE".into());
    }
    let version = match object_member(record, &["Version", "version"]) {
        Some(nrbf::Value::Int32(value)) => *value,
        _ => return Err("ASTRA_EMU_GARBRO_VERSION".into()),
    };
    if !(0..=2).contains(&version) {
        return Err("ASTRA_EMU_GARBRO_VERSION".into());
    }
    let arc_keys =
        object_member(record, &["ArcKeys", "arc_keys"]).ok_or("ASTRA_EMU_GARBRO_ARC_KEYS")?;
    let arc_entries = dictionary_entries(arc_keys)?;
    if arc_entries.len() != REQUIRED_ARCHIVE_ROLES.len()
        || arc_entries.iter().any(|(role, _)| {
            !REQUIRED_ARCHIVE_ROLES
                .iter()
                .any(|expected| role.eq_ignore_ascii_case(expected))
        })
    {
        return Err("ASTRA_EMU_GARBRO_ROLE_SET".into());
    }
    let mut type_entries = Vec::new();
    if let Some(type_keys) = object_member(record, &["TypeKeys", "type_keys"]) {
        type_entries = dictionary_entries(type_keys)?;
    }
    if type_entries.len() > 4
        || type_entries.iter().any(|(key, value)| {
            !["png", "ogg", "sc", "avi"]
                .iter()
                .any(|expected| key.eq_ignore_ascii_case(expected))
                || !matches!(value, nrbf::Value::String(value) if value.len() <= 1024)
        })
    {
        return Err("ASTRA_EMU_GARBRO_TYPE_KEY_SET".into());
    }
    let passwords = type_entries
        .into_iter()
        .map(|(key, value)| match value {
            nrbf::Value::String(value) => Ok((key.to_owned(), (*value).to_owned())),
            _ => Err("ASTRA_EMU_GARBRO_TYPE_KEY_VALUE"),
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let mut roles = BTreeMap::new();
    for role in REQUIRED_ARCHIVE_ROLES {
        let value = arc_entries
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(role))
            .map(|(_, value)| *value)
            .ok_or("ASTRA_EMU_GARBRO_ROLE_MISSING")?;
        if !matches!(value, nrbf::Value::Object(object) if object.class.ends_with("PazKey")) {
            return Err("ASTRA_EMU_GARBRO_ROLE_TYPE".into());
        }
        let index_key = json_bytes(
            object_member(value, &["IndexKey", "index_key"]).ok_or("ASTRA_EMU_GARBRO_INDEX_KEY")?,
        )?;
        let data_value =
            object_member(value, &["DataKey", "data_key"]).ok_or("ASTRA_EMU_GARBRO_DATA_KEY")?;
        let data_key = if role == "mov" && matches!(data_value, nrbf::Value::Null) {
            Vec::new()
        } else {
            json_bytes(data_value)?
        };
        if !(4..=56).contains(&index_key.len())
            || (role != "mov" && !(4..=56).contains(&data_key.len()))
            || data_key.len() > 56
        {
            return Err("ASTRA_EMU_GARBRO_KEY_SIZE".into());
        }
        roles.insert(
            role.into(),
            ImportedRole {
                index_key,
                data_key,
                type_keys: passwords.clone(),
                version,
            },
        );
    }
    Ok(roles)
}
fn json_bytes(value: &nrbf::Value<'_>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let nrbf::Value::Array(values) = value else {
        return Err("ASTRA_EMU_GARBRO_KEY_TYPE".into());
    };
    values
        .iter()
        .map(|value| match value {
            nrbf::Value::Byte(value) => Ok(*value),
            nrbf::Value::UInt16(value) => {
                u8::try_from(*value).map_err(|_| "ASTRA_EMU_GARBRO_KEY_BYTE".into())
            }
            _ => Err("ASTRA_EMU_GARBRO_KEY_BYTE".into()),
        })
        .collect()
}

fn render_patch(
    roles: &BTreeMap<String, ImportedRole>,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut source = String::from("local schemes = {}\n");
    for (role, scheme) in roles {
        let type_keys = scheme
            .type_keys
            .iter()
            .map(|(key, value)| {
                format!(
                    "[{}]={}",
                    bytes_literal(key.as_bytes()),
                    bytes_literal(value.as_bytes())
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        source.push_str(&format!("schemes[{role:?}] = {{ version={}, index_key=buffer.fromstring({}), data_key=buffer.fromstring({}), type_keys={{ {} }} }}\n",scheme.version,bytes_literal(&scheme.index_key),bytes_literal(&scheme.data_key),type_keys));
    }
    source.push_str(r#"
astra.vfs.register_decoder({
  id = "minori.paz", capabilities = { "astra.vfs.decode.v1" },
  decode_index = function(bytes, descriptor)
    local scheme = schemes[descriptor.role]
    if scheme == nil then error("ASTRA_EMU_DECODER_ROLE") end
    return astra.vfs.blowfish(bytes, scheme.index_key, true)
  end,
  decode_entries = function(bytes, entries)
    if #entries ~= 1 then error("ASTRA_EMU_DECODER_ENTRY_COUNT") end
    local entry = entries[1]
    local scheme = schemes[entry.role]
    if scheme == nil then error("ASTRA_EMU_DECODER_ROLE") end
    if entry.version ~= scheme.version then error("ASTRA_EMU_DECODER_VERSION") end
    local password = nil
    local lower = string.lower(entry.name)
    if not entry.packed then
      if string.match(lower, "%.png$") then password = scheme.type_keys.png
      elseif string.match(lower, "%.ogg$") or entry.role == "se" or entry.role == "voice" then password = scheme.type_keys.ogg
      elseif string.match(lower, "%.sc$") then password = scheme.type_keys.sc
      elseif string.match(lower, "%.avi$") or string.match(lower, "%.mpg$") or string.match(lower, "%.mpeg$") then password = scheme.type_keys.avi end
    end
    if entry.role == "mov" then
      local key = astra.vfs.cp932(lower .. string.format(" %08X ", entry.unpacked_size))
      return astra.vfs.mov_decode(bytes, entry.video_key, key, entry.version, entry.chunk_offset, entry.total_size)
    end
    bytes = astra.vfs.blowfish(bytes, scheme.data_key, true)
    if entry.version > 0 and password ~= nil and password ~= "" then
      local key = astra.vfs.cp932(lower .. string.format(" %08X ", entry.unpacked_size) .. password)
      local skip = 0
      if entry.version >= 2 then skip = bit32.band(bit32.rshift(astra.vfs.crc32(key), 12), 0xff) end
      bytes = astra.vfs.rc4(bytes, key, skip + entry.chunk_offset)
    end
    return bytes
  end,
})
"#);
    Ok(source)
}
fn bytes_escape(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("\\x{byte:02x}")).collect()
}
fn bytes_literal(bytes: &[u8]) -> String {
    format!("\"{}\"", bytes_escape(bytes))
}
fn parse_u32(value: &str) -> Result<u32, String> {
    if let Some(hex) = value.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).map_err(|error| error.to_string())
    } else {
        value
            .parse()
            .map_err(|error: std::num::ParseIntError| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, io::Cursor};

    use astra_core::Hash256;
    use astra_emu_family_api::{
        LegacyPackManifest, LegacyProviderError, LegacyVfsEntry, LegacyVfsNode,
        LegacyVfsReadResult, LegacyVfsStat, LegacyVfsStream,
    };

    use super::*;

    struct ExtractFixture {
        manifest: LegacyPackManifest,
        files: BTreeMap<String, Vec<u8>>,
        fail_uri: Option<String>,
    }

    impl ExtractFixture {
        fn new(entries: &[(&str, &[u8])], fail_uri: Option<&str>) -> Self {
            let files = entries
                .iter()
                .map(|(uri, bytes)| ((*uri).to_owned(), (*bytes).to_vec()))
                .collect::<BTreeMap<_, _>>();
            let manifest_entries = entries
                .iter()
                .enumerate()
                .map(|(index, (uri, bytes))| LegacyVfsEntry {
                    uri: (*uri).into(),
                    entry_id: format!("entry-{index}"),
                    offset: 0,
                    size: bytes.len() as u64,
                    content_hash: Hash256::from_sha256(bytes),
                    media_kind: "fixture".into(),
                })
                .collect();
            Self {
                manifest: LegacyPackManifest {
                    mount_id: "extract-fixture".into(),
                    prefix: "minori:/".into(),
                    reader_id: "extract-fixture".into(),
                    reader_hash: Hash256::from_sha256(b"extract-fixture"),
                    entries: manifest_entries,
                },
                files,
                fail_uri: fail_uri.map(str::to_owned),
            }
        }
    }

    impl LegacyMountedVfs for ExtractFixture {
        fn mount_id(&self) -> &str {
            &self.manifest.mount_id
        }

        fn manifest(&self) -> &LegacyPackManifest {
            &self.manifest
        }

        fn read_dir(&self, _uri: &str) -> Result<Vec<LegacyVfsNode>, LegacyProviderError> {
            unreachable!("extract does not enumerate through read_dir")
        }

        fn stat(&self, _uri: &str) -> Result<LegacyVfsStat, LegacyProviderError> {
            unreachable!("extract does not call stat")
        }

        fn read_range(
            &self,
            _uri: &str,
            _offset: u64,
            _length: u64,
        ) -> Result<LegacyVfsReadResult, LegacyProviderError> {
            unreachable!("extract streams complete entries")
        }

        fn open_stream(&self, uri: &str) -> Result<Box<dyn LegacyVfsStream>, LegacyProviderError> {
            if self.fail_uri.as_deref() == Some(uri) {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_EXTRACT_FIXTURE_FAILURE",
                    "fixture stream failed",
                ));
            }
            let bytes = self.files.get(uri).ok_or_else(|| {
                LegacyProviderError::invalid("ASTRA_EMU_VFS_NOT_FOUND", "fixture entry is missing")
            })?;
            Ok(Box::new(Cursor::new(bytes.clone())))
        }
    }

    #[test]
    fn extract_commits_the_complete_tree_atomically() {
        let fixture = ExtractFixture::new(
            &[
                ("minori:/scr/one.sc", b"one"),
                ("minori:/scr/nested/two.sc", b"two"),
            ],
            None,
        );
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("decoded");
        extract(&fixture, &output, Some("scr"), None, None).unwrap();
        assert_eq!(fs::read(output.join("scr/one.sc")).unwrap(), b"one");
        assert_eq!(fs::read(output.join("scr/nested/two.sc")).unwrap(), b"two");
    }

    #[test]
    fn extract_failure_leaves_no_partial_output_or_staging_tree() {
        let fixture = ExtractFixture::new(
            &[
                ("minori:/scr/one.sc", b"one"),
                ("minori:/scr/two.sc", b"two"),
            ],
            Some("minori:/scr/two.sc"),
        );
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("decoded");
        assert!(extract(&fixture, &output, Some("scr"), None, None).is_err());
        assert!(!output.exists());
        assert!(!temp.path().join(".decoded.astra-extract-tmp").exists());
    }

    #[test]
    fn extract_rejects_case_collisions_before_writing() {
        let fixture = ExtractFixture::new(
            &[("minori:/scr/A.sc", b"one"), ("minori:/scr/a.sc", b"two")],
            None,
        );
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("decoded");
        let error = extract(&fixture, &output, Some("scr"), None, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("ASTRA_EMU_MINORI_EXTRACT_CASE_COLLISION"));
        assert!(!output.exists());
    }

    #[test]
    fn patch_renderer_keeps_imported_bytes_out_of_luau_source_literals() {
        let roles = BTreeMap::from([(
            "scr".to_owned(),
            ImportedRole {
                index_key: b"index\n\"key".to_vec(),
                data_key: b"data\\key".to_vec(),
                type_keys: BTreeMap::from([("sc".to_owned(), "private-value-987".to_owned())]),
                version: 2,
            },
        )]);

        let patch = render_patch(&roles).unwrap();
        assert!(patch.is_ascii());
        assert!(patch.contains("astra.vfs.register_decoder"));
        assert!(patch.contains(r#"\x69\x6e\x64\x65\x78\x0a\x22\x6b\x65\x79"#));
        assert!(!patch.contains("private-value-987"));
        assert!(!patch.contains("data\\key"));
    }

    #[test]
    fn byte_literal_escapes_every_byte_including_ascii() {
        assert_eq!(bytes_literal(b"A\0\n\""), r#""\x41\x00\x0a\x22""#);
    }
}
