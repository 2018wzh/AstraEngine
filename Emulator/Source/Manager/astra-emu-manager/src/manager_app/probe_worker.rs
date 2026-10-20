use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const SCAN_PROGRESS_INTERVAL: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScanKind {
    Full,
    Import,
}

pub(super) struct ScanRequest {
    pub(super) kind: ScanKind,
    pub(super) paths: Vec<PathBuf>,
    pub(super) games: Vec<GameRecord>,
    pub(super) excluded: BTreeSet<PathBuf>,
    pub(super) cancel: Arc<AtomicBool>,
}

pub(super) struct GameProbe {
    pub(super) path: PathBuf,
    pub(super) game_ids: Vec<String>,
    pub(super) candidates: Vec<FamilyProbeCandidate>,
    pub(super) patch_profile: Option<Result<astra_emu_patch::OutputProfile, String>>,
}

#[derive(Default)]
pub(super) struct ScanReport {
    pub(super) games: Vec<GameProbe>,
    pub(super) missing: usize,
    pub(super) cache_hits: usize,
    pub(super) cache_error: Option<String>,
}

pub(super) fn scan(
    registry: &FamilyProviderRegistry,
    cache: &mut probe_cache::ProbeCache,
    request: ScanRequest,
    shutdown: &AtomicBool,
    progress: &mut dyn FnMut(super::owner::PrepareProgress),
) -> Result<ScanReport, String> {
    let cancelled = || {
        if request.cancel.load(Ordering::Acquire) {
            Some("ASTRA_EMU_SCAN_CANCELLED")
        } else if shutdown.load(Ordering::Acquire) {
            Some("ASTRA_EMU_OWNER_CANCELLED")
        } else {
            None
        }
    };
    let excluded: BTreeSet<PathBuf> = request
        .excluded
        .iter()
        .map(|path| fs::canonicalize(path).unwrap_or_else(|_| path.clone()))
        .collect();
    let mut report = ScanReport::default();
    let mut known = BTreeMap::<PathBuf, Vec<String>>::new();
    for game in &request.games {
        if let Some(error) = cancelled() {
            return Err(error.into());
        }
        match fs::canonicalize(&game.location) {
            Ok(path) if excluded.contains(&path) => continue,
            Ok(path) if path.is_dir() && !astra_emu_patch::is_staging_path(&path) => {
                known.entry(path).or_default().push(game.game_id.clone())
            }
            Ok(path) if astra_emu_patch::is_staging_path(&path) => continue,
            _ => {
                report.missing += 1;
                report.games.push(GameProbe {
                    path: PathBuf::from(&game.location),
                    game_ids: vec![game.game_id.clone()],
                    candidates: Vec::new(),
                    patch_profile: None,
                });
            }
        }
    }
    let mut roots = BTreeSet::new();
    if request.kind == ScanKind::Full {
        roots.extend(known.keys().cloned());
        cache.retain_registered(&request.games);
    }
    for root in request.paths {
        let path = match fs::canonicalize(root) {
            Ok(path) if astra_emu_patch::is_staging_path(&path) => {
                if request.kind == ScanKind::Full {
                    continue;
                }
                return Err(
                    "ASTRA_EMU_PATCH_INVALID_REQUEST: private staging cannot be imported".into(),
                );
            }
            Ok(path) if path.is_dir() => path,
            _ if request.kind == ScanKind::Full => {
                report.missing += 1;
                continue;
            }
            _ => return Err("ASTRA_EMU_GAME_DIRECTORY_INVALID".into()),
        };
        roots.insert(path);
    }
    let mut visited = BTreeSet::new();
    let mut recognized = Vec::<PathBuf>::new();
    for root in roots {
        let mut queue = vec![(root, 0_usize)];
        while let Some((path, depth)) = queue.pop() {
            if let Some(error) = cancelled() {
                return Err(error.into());
            }
            if excluded.contains(&path)
                || (!known.contains_key(&path)
                    && recognized.iter().any(|game| path.starts_with(game)))
                || !visited.insert(path.clone())
            {
                continue;
            }
            if visited.len() % SCAN_PROGRESS_INTERVAL == 0 {
                progress(super::owner::PrepareProgress::Scan {
                    directories: visited.len(),
                    games: recognized.len(),
                });
            }
            if visited.len() > MAX_SCAN_DIRECTORIES {
                return Err("ASTRA_EMU_GAME_DIRECTORY_BOUNDS".into());
            }
            let allow_hint = request.kind == ScanKind::Full && known.contains_key(&path);
            let (candidates, hit) = cache.probe(&path, registry, allow_hint)?;
            report.cache_hits += usize::from(hit);
            let matched = !candidates.is_empty();
            let patch_profile = if request.kind == ScanKind::Import {
                match astra_emu_patch::read_output_profile(&path, shutdown) {
                    Ok(Some(profile)) => Some(Ok(profile)),
                    Ok(None) => None,
                    Err(error) => Some(Err(error.to_string())),
                }
            } else {
                None
            };
            let has_receipt = patch_profile.is_some();
            if matched || known.contains_key(&path) || has_receipt {
                report.games.push(GameProbe {
                    path: path.clone(),
                    game_ids: known.get(&path).cloned().unwrap_or_default(),
                    candidates,
                    patch_profile,
                });
            }
            if matched || has_receipt {
                recognized.push(path);
            } else if depth < MAX_SCAN_DEPTH {
                let mut children = Vec::new();
                for entry in
                    fs::read_dir(&path).map_err(|_| "ASTRA_EMU_GAME_DIRECTORY_ENUMERATION")?
                {
                    let entry = entry.map_err(|_| "ASTRA_EMU_GAME_DIRECTORY_ENUMERATION")?;
                    let kind = entry
                        .file_type()
                        .map_err(|_| "ASTRA_EMU_GAME_DIRECTORY_ENUMERATION")?;
                    let child = entry.path();
                    if kind.is_dir()
                        && !kind.is_symlink()
                        && !astra_emu_patch::is_staging_path(&child)
                    {
                        children.push(child);
                        if children.len() + queue.len() + visited.len() > MAX_SCAN_DIRECTORIES {
                            return Err("ASTRA_EMU_GAME_DIRECTORY_BOUNDS".into());
                        }
                    }
                }
                children.sort();
                queue.extend(children.into_iter().rev().map(|path| (path, depth + 1)));
            }
        }
    }
    if let Some(error) = cancelled() {
        return Err(error.into());
    }
    report.cache_error = cache.save().err();
    Ok(report)
}
