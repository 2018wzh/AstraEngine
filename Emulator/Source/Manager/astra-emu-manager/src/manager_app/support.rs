use super::*;

pub(super) const NETWORK_ALLOWED_KEY: &str = "network.allowed";
pub(super) const TRANSLATION_ENABLED_KEY: &str = "translation.enabled";
pub(super) const SENSITIVE_COVERS_KEY: &str = "covers.sensitive";
pub(super) const BANGUMI_SECRET_REFERENCE: &str = "metadata.bangumi";

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

#[cfg(not(target_os = "android"))]
pub(super) fn core_directories(user_cores: &Path) -> Result<Vec<PathBuf>, String> {
    let executable = env::current_exe().map_err(|_| "ASTRA_EMU_EXECUTABLE_LOCATION")?;
    let bundled = executable
        .parent()
        .ok_or("ASTRA_EMU_EXECUTABLE_LOCATION")?
        .join("cores");
    let mut directories = vec![user_cores.to_owned()];
    if bundled.is_dir() {
        directories.push(bundled);
    }
    Ok(directories)
}

pub(super) fn sha256_id(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(value.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
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
        format!("{} 天前", seconds / 86_400)
    } else if seconds >= 3600 {
        format!("{} 小时前", seconds / 3600)
    } else if seconds >= 60 {
        format!("{} 分钟前", seconds / 60)
    } else {
        "刚刚".into()
    }
}

pub(super) fn local_time_label(unix_ms: i64) -> String {
    let unix_s = unix_ms.div_euclid(1000);
    let days = unix_s.div_euclid(86_400);
    let secs = unix_s.rem_euclid(86_400);
    let hour = secs / 3600;
    let minute = (secs % 3600) / 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} UTC")
}

fn civil_from_days(mut z: i64) -> (i32, u32, u32) {
    z += 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year + if month <= 2 { 1 } else { 0 };
    (year as i32, month, day)
}

#[cfg(test)]
mod tests {
    use super::{human_relative, local_time_label};

    #[test]
    fn local_time_label_formats_unix_epoch_in_utc() {
        assert_eq!(local_time_label(0), "1970-01-01 00:00 UTC");
        assert_eq!(local_time_label(1_700_000_000_000), "2023-11-14 22:13 UTC");
    }

    #[test]
    fn human_relative_uses_chinese_units() {
        assert_eq!(human_relative(0, 3 * 86_400_000), "3 天前");
        assert_eq!(human_relative(0, 30_000), "刚刚");
    }
}
