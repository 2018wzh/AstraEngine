use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScanKind {
    Full,
    Import,
}

pub(super) struct ScanRequest {
    pub(super) kind: ScanKind,
    pub(super) paths: Vec<PathBuf>,
    pub(super) games: Vec<GameRecord>,
}

pub(super) struct GameProbe {
    pub(super) path: PathBuf,
    pub(super) game_ids: Vec<String>,
    pub(super) candidates: Vec<FamilyProbeCandidate>,
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
    cancelled: &AtomicBool,
) -> Result<ScanReport, String> {
    let mut report = ScanReport::default();
    let mut known = BTreeMap::<PathBuf, Vec<String>>::new();
    for game in &request.games {
        if cancelled.load(Ordering::Acquire) {
            return Err("ASTRA_EMU_OWNER_CANCELLED".into());
        }
        match fs::canonicalize(&game.location) {
            Ok(path) if path.is_dir() => known.entry(path).or_default().push(game.game_id.clone()),
            _ => {
                report.missing += 1;
                report.games.push(GameProbe {
                    path: PathBuf::from(&game.location),
                    game_ids: vec![game.game_id.clone()],
                    candidates: Vec::new(),
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
            if cancelled.load(Ordering::Acquire) {
                return Err("ASTRA_EMU_OWNER_CANCELLED".into());
            }
            if (!known.contains_key(&path) && recognized.iter().any(|game| path.starts_with(game)))
                || !visited.insert(path.clone())
            {
                continue;
            }
            if visited.len() > MAX_SCAN_DIRECTORIES {
                return Err("ASTRA_EMU_GAME_DIRECTORY_BOUNDS".into());
            }
            let allow_hint = request.kind == ScanKind::Full && known.contains_key(&path);
            let (candidates, hit) = cache.probe(&path, registry, allow_hint)?;
            report.cache_hits += usize::from(hit);
            let matched = !candidates.is_empty();
            if matched || known.contains_key(&path) {
                report.games.push(GameProbe {
                    path: path.clone(),
                    game_ids: known.get(&path).cloned().unwrap_or_default(),
                    candidates,
                });
            }
            if matched {
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
                    if kind.is_dir() && !kind.is_symlink() {
                        children.push(entry.path());
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
    if cancelled.load(Ordering::Acquire) {
        return Err("ASTRA_EMU_OWNER_CANCELLED".into());
    }
    report.cache_error = cache.save().err();
    Ok(report)
}
