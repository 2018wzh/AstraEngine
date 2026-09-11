use super::*;

pub(super) fn unix_time_ms() -> Result<i64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "ASTRA_EMU_SYSTEM_CLOCK_INVALID".to_owned())?
        .as_millis()
        .try_into()
        .map_err(|_| "ASTRA_EMU_SYSTEM_CLOCK_OVERFLOW".to_owned())
}

pub(super) fn platform_data_dir() -> Result<PathBuf, String> {
    if let Some(value) = env::var_os("ASTRA_EMU_DATA_DIR") {
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err("ASTRA_EMU_DATA_DIRECTORY_INVALID".into());
        }
        return Ok(path);
    }
    directories::ProjectDirs::from("dev", "AstraEngine", "AstraEMU")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .ok_or_else(|| "ASTRA_EMU_DATA_DIRECTORY_UNAVAILABLE".into())
}

pub(super) fn sha256_id(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(value.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn sha256_bytes_id(value: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(super) fn compatibility_cache_path(data_dir: &Path) -> PathBuf {
    data_dir.join("compatibility.json")
}

pub(super) fn cover_extension(media_type: &str) -> Result<&'static str, String> {
    match media_type {
        "image/png" => Ok("png"),
        "image/jpeg" => Ok("jpg"),
        "image/webp" => Ok("webp"),
        "image/gif" => Ok("gif"),
        "image/bmp" => Ok("bmp"),
        _ => Err("ASTRA_EMU_METADATA_COVER_MEDIA_TYPE".into()),
    }
}

pub(super) fn store_cover(
    data_dir: &Path,
    game_id: &str,
    cover: &CoverAsset,
) -> Result<(), String> {
    let directory = data_dir.join("covers");
    fs::create_dir_all(&directory).map_err(|_| "ASTRA_EMU_METADATA_COVER_DIRECTORY")?;
    let extension = cover_extension(&cover.media_type)?;
    for previous in ["png", "jpg", "webp", "gif", "bmp"] {
        let path = directory.join(format!("{game_id}.{previous}"));
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("ASTRA_EMU_METADATA_COVER_REMOVE".into()),
        }
    }
    fs::write(
        directory.join(format!("{game_id}.{extension}")),
        &cover.bytes,
    )
    .map_err(|_| "ASTRA_EMU_METADATA_COVER_WRITE".into())
}

pub(super) fn load_compatibility_cache(
    data_dir: &Path,
) -> Result<(Option<CompatibilityDatabase>, Option<String>), String> {
    let path = compatibility_cache_path(data_dir);
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((None, None)),
        Err(_) => return Err("ASTRA_EMU_COMPATIBILITY_CACHE_READ".into()),
    };
    let database: CompatibilityDatabase =
        serde_json::from_slice(&bytes).map_err(|_| "ASTRA_EMU_COMPATIBILITY_CACHE_INVALID")?;
    database
        .validate()
        .map_err(|_| "ASTRA_EMU_COMPATIBILITY_CACHE_INVALID")?;
    let hash = sha256_bytes_id(&bytes);
    Ok((Some(database), Some(hash)))
}

pub(super) fn save_compatibility_cache(
    data_dir: &Path,
    database: &CompatibilityDatabase,
) -> Result<String, String> {
    database
        .validate()
        .map_err(|_| "ASTRA_EMU_COMPATIBILITY_CACHE_INVALID")?;
    let bytes =
        serde_json::to_vec(database).map_err(|_| "ASTRA_EMU_COMPATIBILITY_CACHE_SERIALIZATION")?;
    let path = compatibility_cache_path(data_dir);
    use std::io::Write;
    let mut temporary = tempfile::NamedTempFile::new_in(data_dir)
        .map_err(|_| "ASTRA_EMU_COMPATIBILITY_CACHE_CREATE")?;
    temporary
        .write_all(&bytes)
        .map_err(|_| "ASTRA_EMU_COMPATIBILITY_CACHE_WRITE")?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| "ASTRA_EMU_COMPATIBILITY_CACHE_SYNC")?;
    temporary
        .persist(&path)
        .map_err(|_| "ASTRA_EMU_COMPATIBILITY_CACHE_REPLACE")?;
    Ok(sha256_bytes_id(&bytes))
}

pub(super) fn path_id(path: &Path) -> String {
    format!("game-{}", sha256_id(&path.to_string_lossy()))
}

pub(super) fn title_for_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Untitled game")
        .to_owned()
}

pub(super) fn parse_game_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for name in ["ASTRA_EMU_GAME_DIR", "ASTRA_EMU_GAME_DIRS"] {
        if let Some(value) = env::var_os(name) {
            roots.extend(env::split_paths(&value));
        }
    }
    roots
}

pub(super) fn enumerate_directories(root: &Path) -> Result<Vec<PathBuf>, String> {
    let root = fs::canonicalize(root).map_err(|_| "ASTRA_EMU_GAME_DIRECTORY_INVALID".to_owned())?;
    if !root.is_dir() {
        return Err("ASTRA_EMU_GAME_DIRECTORY_NOT_DIRECTORY".into());
    }
    let mut result = Vec::new();
    let mut queue = vec![(root, 0_usize)];
    while let Some((directory, depth)) = queue.pop() {
        if result.len() >= MAX_SCAN_DIRECTORIES {
            return Err("ASTRA_EMU_GAME_DIRECTORY_BOUNDS".into());
        }
        result.push(directory.clone());
        if depth >= MAX_SCAN_DEPTH {
            continue;
        }
        for entry in fs::read_dir(&directory)
            .map_err(|_| "ASTRA_EMU_GAME_DIRECTORY_ENUMERATION".to_owned())?
        {
            let entry = entry.map_err(|_| "ASTRA_EMU_GAME_DIRECTORY_ENUMERATION".to_owned())?;
            let file_type = entry
                .file_type()
                .map_err(|_| "ASTRA_EMU_GAME_DIRECTORY_ENUMERATION".to_owned())?;
            if file_type.is_dir() && !file_type.is_symlink() {
                queue.push((entry.path(), depth + 1));
            }
        }
    }
    result.sort();
    result.dedup();
    Ok(result)
}

pub(super) fn human_duration(ms: i64) -> String {
    let seconds = (ms.max(0) / 1000) as u64;
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        format!("{seconds}s")
    }
}
pub(super) fn human_relative(timestamp: i64, now: i64) -> String {
    let seconds = ((now.saturating_sub(timestamp)) / 1000).max(0);
    if seconds >= 86_400 {
        format!("{}d ago", seconds / 86_400)
    } else if seconds >= 3600 {
        format!("{}h ago", seconds / 3600)
    } else if seconds >= 60 {
        format!("{}m ago", seconds / 60)
    } else {
        "just now".into()
    }
}
pub(super) fn compatibility_status_label(status: &str) -> String {
    match status {
        "perfect" => "Perfect",
        "completable" => "Completable",
        "flawed" => "Flawed",
        "boot_only" => "Boot only",
        "unplayable" => "Unplayable",
        other => other,
    }
    .into()
}
pub(super) fn profile_kind(profile: Option<TranslationProfile>) -> String {
    profile
        .map(|profile| {
            match profile.endpoint_kind {
                TranslationEndpointKind::OpenAiCompatible => "openai-compatible",
                TranslationEndpointKind::OpenAi => "openai",
                TranslationEndpointKind::Ecnu => "ecnu",
                TranslationEndpointKind::ThirdParty => "third_party",
            }
            .to_owned()
        })
        .unwrap_or_else(|| "openai-compatible".into())
}
