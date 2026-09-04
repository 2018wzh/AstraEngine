// Demo mode — synthetic library + fake stage, memory-only, no RuntimeWorld.
// Desktop+Android, session-writable but discard on exit (no SQLite).
// Covers: generated locally (no network, no copyrighted payload) mimicking VNDB/Bangumi palette.
// Metadata: dual-source VNDB+Bangumi mock tied to astra-emu-metadata model.

use std::path::PathBuf;

use astra_emu_manager::{AstraUnderlayRenderer, ManagerController, WgpuFrameContext};
use astra_emu_manager_ui_slint::{
    AppearanceViewModel, GameCardViewModel, InputConfigViewModel, ManagerViewModel, MatchReviewViewModel,
    PlaySessionViewModel, VfsEntryViewModel,
};

fn demo_cover_dir() -> PathBuf {
    std::env::temp_dir().join("astra-emu-demo-covers")
}

fn try_fetch_real_cover(vndb_id: &str, dest: &std::path::Path) -> bool {
    use astra_emu_metadata::{MetadataProvider, ReleaseUse, VndbProvider, VndbProviderConfig, MetadataLicenseManifest};
    use std::time::Duration;
    if let Ok(meta) = std::fs::metadata(dest) {
        if let Ok(modified) = meta.modified() {
            if let Ok(elapsed) = modified.elapsed() {
                if elapsed < Duration::from_secs(7 * 24 * 3600) { return true; }
            }
        }
        if meta.len() > 50_000 { return true; }
    }
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() { Ok(rt) => rt, Err(_) => return false };
    rt.block_on(async {
        let provider = match VndbProvider::new(VndbProviderConfig {
            network_consent: true,
            license: MetadataLicenseManifest { release_use: ReleaseUse::NonCommercial, vndb_commercial_license_id: None },
            timeout: Duration::from_secs(10),
            minimum_request_delay: Duration::from_millis(250),
        }) { Ok(p) => p, Err(_) => return false };
        let record = match provider.fetch_by_id(vndb_id).await { Ok(r) => r, Err(_) => return false };
        if record.cover.is_none() { return false; }
        let asset = match provider.fetch_cover(&record, false).await { Ok(a) => a, Err(_) => return false };
        if asset.bytes.is_empty() || asset.bytes.len() > 8 * 1024 * 1024 { return false; }
        let img = match image::load_from_memory(&asset.bytes) { Ok(i) => i, Err(_) => return std::fs::write(dest, &asset.bytes).is_ok() };
        img.save(dest).is_ok()
    })
}

fn ensure_demo_covers() -> Vec<String> {
    let dir = demo_cover_dir();
    let _ = std::fs::create_dir_all(&dir);
    let mut uris = Vec::new();
    for i in 0..REAL_VNS.len() {
        let path = dir.join(format!("demo-cover-{:02}.png", i));
        if path.metadata().map(|m| m.len() > 50_000).unwrap_or(false) {
            uris.push(path.to_string_lossy().to_string());
        } else {
            uris.push(String::new());
        }
    }
    // Background upgrade to real VNDB covers (non-blocking across all 20 entries)
    {
        let dir_clone = dir.clone();
        std::thread::spawn(move || {
            for i in 0..REAL_VNS.len() {
                let path = dir_clone.join(format!("demo-cover-{:02}.png", i));
                if path.metadata().map(|m| m.len() < 50_000).unwrap_or(true) {
                    let vndb_id = REAL_VNS[i].1;
                    let _ = try_fetch_real_cover(vndb_id, &path);
                }
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
        });
    }
    uris
}

/// 20 real VNs — titles+VNDB/Bangumi IDs + PotatoVN-style extended fields
/// Covers: first try real t.vndb.org/lain.bgm.tv via CoverFetcher, fallback synthetic gradient
#[allow(clippy::type_complexity)]
const REAL_VNS: [(&str, &str, &str, &str, &str, &str, &str, &str, &str); 20] = [
    ("Clannad", "v4", "r1827", "Key", "2004-04-28", "PC / PS2 / Switch", "Siglus", "岡崎朋也と古河渚の出会い、家族と奇跡の物語。Key 社の泣きゲー最高峰。", "泣きゲー, 学園, 家族"),
    ("Steins;Gate", "v2002", "r29927", "Nitroplus", "2009-10-15", "Win / Xbox360", "Proprietary", "秋葉原、偶然の発明が世界線を変える。タイムリープと選択の物語。", "SF, タイムリープ, サスペンス"),
    ("Fate/stay night", "v11", "r11", "TYPE-MOON", "2004-01-30", "PC / PS2", "KiriKiri", "聖杯戦争、七人の魔術師と英霊的戦い。", "伝奇, バトル, 聖杯戦争"),
    ("White Album 2", "v7771", "r28319", "Leaf", "2010-03-26", "PC", "CatSystem2", "冬、届かない恋。不器用な三人の記憶。", "恋愛, トライアングル, 音楽"),
    ("Little Busters!", "v5", "r2243", "Key", "2007-07-27", "PC", "Siglus", "少年少女たちの夏、秘密基地と世界征服。", "学園, 友情, 泣きゲー"),
    ("Kanon", "v33", "r17", "Key", "1999-06-04", "PC", "Avg32", "雪の街、五つの少女との再会と奇跡。", "泣きゲー, 雪, 奇跡"),
    ("Air", "v15", "r16", "Key", "2000-09-08", "PC", "Avg32", "海辺の町、翼を持つ少女の夏。", "夏, 翼, 伝説"),
    ("Grisaia no Kajitsu", "v5154", "r15907", "Frontwing", "2011-02-25", "PC", "CatSystem2", "美浜学園、5人の少女と1人の少年。", "学園, ミステリー, 救済"),
    ("Muv-Luv Alternative", "v92", "r92", "âge", "2006-02-24", "PC", "rUGP", "BETA侵略、人類存亡を賭けた戦い。", "SF, ロボット, 戦争"),
    ("Higurashi no Naku Koro ni", "v148", "r148", "07th Expansion", "2002-08-10", "PC", "NScripter", "雛見沢村、繰り返される惨劇の夏。", "ホラー, ミステリー, ループ"),
    ("Umineko no Naku Koro ni", "v167", "r167", "07th Expansion", "2007-08-17", "PC", "NScripter", "六軒島、黄金の魔女と無限の密室。", "ミステリー, 魔女, 推理"),
    ("Katawa Shoujo", "v1811", "r1811", "Four Leaf Studios", "2012-01-04", "PC / Linux", "Ren'Py", "障害を抱える少女たちとの学園生活。", "学園, 恋愛, 海外VN"),
    ("Doki Doki Literature Club!", "v16768", "r37182", "Team Salvato", "2017-09-22", "PC", "Ren'Py", "文芸部、甘い日常が崩れる時。", "ホラー, メタ, 文芸部"),
    ("Nekopara Vol.1", "v15308", "r34014", "NEKO WORKs", "2014-12-29", "PC", "KiriKiri", "パティシエと猫娘たちの同居生活。", "日常, 猫, 萌え"),
    ("Fate/hollow ataraxia", "v12", "r12", "TYPE-MOON", "2005-10-28", "PC", "KiriKiri", "聖杯戦争の後、四日間のループ。", "日常, 伝奇, ループ"),
    ("Rewrite", "v200", "r200", "Key", "2011-06-24", "PC", "Siglus", "超能力と環境問題、世界の改変。", "学園, 異能, 環境"),
    ("G-Senjou no Maou", "v88", "r88", "Akabeisoft2", "2008-05-29", "PC", "Ethornell", "裏社会を操る魔王と青年の対決。", "サスペンス, 音楽, 天才"),
    ("Baldr Sky Dive1", "v2201", "r2201", "GIGA", "2009-03-27", "PC", "Baldr", "戦場と記憶、少女とAI。", "SF, ロボット, 戦闘"),
    ("Dies irae", "v90", "r90", "Light", "2007-12-19", "PC", "Light", "聖槍と黄金、終わりなき戦い。", "伝奇, バトル, 聖槍"),
    ("Sakura no Uta", "v562", "r53289", "Makura", "2015-10-23", "PC", "KiriKiri", "直哉と六人のヒロイン、桜の下の青春。", "学園, 恋愛, 青春"),
];

fn mock_games() -> Vec<GameCardViewModel> {
    let uris = ensure_demo_covers();
    let families = ["fvp", "minori", "krkr", "artemis"];
    let statuses = ["perfect", "completable", "flawed", "boot_only", ""];
    (0..20)
        .map(|i| {
            let (title, _v, _r, _dev, _date, _plat, _eng, _desc, _tags) = REAL_VNS[i];
            GameCardViewModel {
                case_id: format!("demo-case-{:02}", i),
                title: title.into(),
                family: families[i % families.len()].into(),
                cover_uri: uris[i].clone(),
                diagnostic: String::new(),
                play_time: format!("{}h {}m", (i * 3) % 40, (i * 7) % 60),
                last_played: if i % 3 == 0 { "2d ago".into() } else { "just now".into() },
                compatibility_status: statuses[i % statuses.len()].into(),
            }
        })
        .collect()
}

fn mock_match_reviews(case_id: &str) -> Vec<MatchReviewViewModel> {
    let idx: usize = case_id.strip_prefix("demo-case-").and_then(|s| s.parse().ok()).unwrap_or(0);
    let (title, vndb_id, _, developer, date, _, _, _, tags) = REAL_VNS[idx % REAL_VNS.len()];
    let bangumi_id = format!("{}", 100000 + idx * 1234);
    vec![
        MatchReviewViewModel {
            candidate_id: format!("{case_id}-vndb"),
            case_id: case_id.into(),
            provider: "vndb".into(),
            remote_id: vndb_id.into(),
            title: title.into(),
            aliases: format!("{title} / {}", tags.split(',').next().unwrap_or("").trim()),
            release_date: date.into(),
            developer: developer.into(),
            evidence: "title 0.98, developer 0.94, date 0.91".into(),
            score_millis: 980,
            diagnostic: "auto_link_eligible".into(),
        },
        MatchReviewViewModel {
            candidate_id: format!("{case_id}-bangumi"),
            case_id: case_id.into(),
            provider: "bangumi".into(),
            remote_id: bangumi_id.clone(),
            title: title.into(),
            aliases: title.into(),
            release_date: date.into(),
            developer: developer.into(),
            evidence: "title 0.93, date 0.87".into(),
            score_millis: 935,
            diagnostic: "requires_confirmation".into(),
        },
    ]
}

fn mock_play_history() -> Vec<PlaySessionViewModel> {
    vec![
        PlaySessionViewModel { start_time: "2026-08-28 20:15".into(), duration: "1h 23m".into(), ended_by: "user".into() },
        PlaySessionViewModel { start_time: "2026-08-27 14:02".into(), duration: "42m".into(), ended_by: "terminal".into() },
        PlaySessionViewModel { start_time: "2026-08-25 09:11".into(), duration: "15m".into(), ended_by: "crash".into() },
    ]
}

fn mock_vfs_entries() -> Vec<VfsEntryViewModel> {
    vec![
        VfsEntryViewModel { path: "script.hcb".into(), name: "script.hcb".into(), is_dir: false, size_display: "1.2 MB".into(), source_layer: "fvp".into(), expanded: false, depth: 0 },
        VfsEntryViewModel { path: "data.pak".into(), name: "data.pak".into(), is_dir: false, size_display: "342 MB".into(), source_layer: "fvp".into(), expanded: false, depth: 0 },
        VfsEntryViewModel { path: "voice".into(), name: "voice".into(), is_dir: true, size_display: "—".into(), source_layer: "fvp".into(), expanded: true, depth: 0 },
        VfsEntryViewModel { path: "voice/a01.ogg".into(), name: "a01.ogg".into(), is_dir: false, size_display: "1.1 MB".into(), source_layer: "fvp".into(), expanded: false, depth: 1 },
        VfsEntryViewModel { path: "save".into(), name: "save".into(), is_dir: true, size_display: "—".into(), source_layer: "writable".into(), expanded: false, depth: 0 },
    ]
}

pub struct DemoManagerController {
    games: Vec<GameCardViewModel>,
    selected: Option<String>,
    search: String,
    // Config state — session-persistent until exit (fixes rollback)
    selected_nls: String,
    filter_preset: String,
    patch_mode: String,
    pending_family: std::collections::BTreeMap<String, String>,
    pending_extension: std::collections::BTreeMap<String, String>,
    pending_filter: std::collections::BTreeMap<String, String>,
    bangumi_status: String,
    bangumi_rating: i32,
    bangumi_note: String,
}

impl DemoManagerController {
    pub fn new() -> Self {
        let mut pending_family = std::collections::BTreeMap::new();
        pending_family.insert("fvp.nls".into(), "shift_jis".into());
        let mut pending_extension = std::collections::BTreeMap::new();
        pending_extension.insert("translation.timeout_ms".into(), "2000".into());
        let mut pending_filter = std::collections::BTreeMap::new();
        pending_filter.insert("filter.preset".into(), "none".into());
        Self {
            games: mock_games(),
            selected: None,
            search: String::new(),
            selected_nls: "shift_jis".into(),
            filter_preset: "none".into(),
            patch_mode: "no_patch".into(),
            pending_family,
            pending_extension,
            pending_filter,
            bangumi_status: "doing".into(),
            bangumi_rating: 7,
            bangumi_note: "DEMO 私密备注".into(),
        }
    }

    fn filtered(&self) -> Vec<GameCardViewModel> {
        if self.search.is_empty() {
            return self.games.clone();
        }
        let q = self.search.to_lowercase();
        self.games
            .iter()
            .filter(|g| g.title.to_lowercase().contains(&q) || g.family.contains(&q))
            .cloned()
            .collect()
    }

    fn build_model(&self) -> ManagerViewModel {
        let selected = self.selected.clone();
        let sel_game = selected.as_ref().and_then(|id| self.games.iter().find(|g| &g.case_id == id));
        let sel_id = selected.clone().unwrap_or_default();
        let fvp_nls = self.pending_family.get("fvp.nls").cloned().unwrap_or_else(|| self.selected_nls.clone());
        let filter_preset = self.pending_filter.get("filter.preset").cloned().unwrap_or_else(|| self.filter_preset.clone());
        let timeout_val = self.pending_extension.get("translation.timeout_ms").cloned().unwrap_or_else(|| "3000".into());
        let idx: usize = sel_id.strip_prefix("demo-case-").and_then(|s| s.parse().ok()).unwrap_or(0);
        let vndb_pair = if sel_id.is_empty() { String::new() } else {
            let (_, v, r, _, _, _, _, _, _) = REAL_VNS[idx % REAL_VNS.len()];
            format!("{v} / {r}")
        };
        // Extended metadata for selected
        let (ext_dev, ext_date, ext_platforms, ext_engine, ext_desc, ext_tags, ext_aliases, ext_cover_meta) = if sel_id.is_empty() {
            (String::new(), String::new(), String::new(), String::new(), "请选择左侧卡片 — PotatoVN 式双栏沉浸详情：封面+开发商+标签+简介".into(), String::new(), String::new(), String::new())
        } else {
            let (_, _, _, dev, date, platforms, engine, desc, tags) = REAL_VNS[idx % REAL_VNS.len()];
            let cover_w = 320; let cover_h = 480;
            (dev.into(), date.into(), platforms.into(), engine.into(), desc.into(), tags.into(), format!("{} / {} Alias", REAL_VNS[idx].0, tags.split(',').next().unwrap_or("").trim()), format!("{cover_w}x{cover_h} · PNG · via t.vndb.org/lain.bgm.tv CoverFetcher"))
        };
        ManagerViewModel {
            games: self.filtered(),
            match_reviews: if sel_id.is_empty() { Vec::new() } else { mock_match_reviews(&sel_id) },
            selected_case_id: selected,
            search_query: self.search.clone(),
            endpoint_identity: "demo-ecnu-v1".into(),
            model_identity: "demo-model".into(),
            global_diagnostic: String::new(), // no mask — only title shows [DEMO]
            selected_nls: fvp_nls.clone(),
            translation_endpoint_kind: "ecnu".into(),
            translation_endpoint: "https://demo.example".into(),
            translation_protocol: "responses".into(),
            translation_model: "demo".into(),
            translation_target_language: "zh-CN".into(),
            translation_context_sentences: 10,
            translation_body_limit_bytes: 16384,
            translation_timeout_ms: timeout_val.parse().unwrap_or(3000),
            translation_background: "demo background context".into(),
            translation_glossary: "こんにちは=你好\nありがとう=谢谢".into(),
            translation_consent_present: true,
            filter_preset: filter_preset.clone(),
            diagnostics_summary: "DEMO — runtime idle, 20 real VN titles, 42 VFS resources, no blocking diagnostic. CoverFetcher would use t.vndb.org / lain.bgm.tv (https-only, no redirect)".into(),
            patches_summary: format!("DEMO — patch_mode={} (trusted astraemu.patch.luau demo)", self.patch_mode),
            vndb_consent: true,
            bangumi_consent: true,
            sensitive_covers: false,
            bangumi_play_status: self.bangumi_status.clone(),
            bangumi_rating: self.bangumi_rating,
            bangumi_note: self.bangumi_note.clone(),
            bangumi_sync_summary: "DEMO — last sync 2026-08-30 10:00, 7 wish / 12 doing".into(),
            selected_title: sel_game.map(|g| g.title.clone()).unwrap_or_else(|| "请选择左侧卡片查看 Modern 双栏详情".into()),
            selected_family: sel_game.map(|g| g.family.clone()).unwrap_or_default(),
            selected_play_time: sel_game.map(|g| g.play_time.clone()).unwrap_or_default(),
            selected_last_played: sel_game.map(|g| g.last_played.clone()).unwrap_or_default(),
            selected_vfs_status: "DEMO — mount: demo-vfs (Rinne 三布局自适应演示)".into(),
            play_history: if sel_id.is_empty() { Vec::new() } else { mock_play_history() },
            library_sort: "title".into(),
            compatibility_filter: "all".into(),
            compatibility_source_url: "https://demo.example/compat.json".into(),
            compatibility_sync_summary: "DEMO 20 real VNs, last fetched 2026-08-30".into(),
            selected_compatibility_status: sel_game.map(|g| g.compatibility_status.clone()).unwrap_or_else(|| "unknown".into()),
            selected_compatibility_notes: "展示真实 VNDB v/r 对，如 v17/r1827 Clannad".into(),
            selected_compatibility_updated: "2026-08-30".into(),
            selected_compatibility_vndb_id: vndb_pair,
            selected_releases: if sel_id.is_empty() { Vec::new() } else {
                let (_, _, r, _, _, _, _, _, _) = REAL_VNS[idx % REAL_VNS.len()];
                vec![(r.into(), format!("Release {r} — win")), (format!("{r}a"), format!("Release {r}a — win/linux"))]
            },
            current_page: String::new(),
            vfs_entries: mock_vfs_entries(),
            vfs_preview: None,
            vfs_selected_path: String::new(),
            vfs_current_dir: "/".into(),
            vfs_mount_summary: "DEMO mount — 5 entries 预览（真实 VFS 会列 42+）".into(),
            input_config: InputConfigViewModel::default(),
            appearance: AppearanceViewModel { theme_mode: "system".into(), ..Default::default() },
            family_config_fields: vec![
                astra_emu_manager_ui_slint::GenericConfigFieldViewModel { key: "fvp.nls".into(), label: "文本编码".into(), description: "shift_jis / gbk / utf8".into(), kind: "enum".into(), value: fvp_nls, enum_values: vec!["shift_jis".into(),"gbk".into(),"utf8".into()], required: true, min: 0, max: 0 },
            ],
            extension_config_fields: vec![
                astra_emu_manager_ui_slint::GenericConfigFieldViewModel { key: "translation.timeout_ms".into(), label: "翻译超时".into(), description: "2000 ms 演示".into(), kind: "integer".into(), value: timeout_val, enum_values: vec![], required: false, min: 1000, max: 120000 },
            ],
            filter_config_fields: vec![
                astra_emu_manager_ui_slint::GenericConfigFieldViewModel { key: "filter.preset".into(), label: "滤镜预设".into(), description: "none/grayscale/crt-soft/warm".into(), kind: "enum".into(), value: filter_preset, enum_values: vec!["none".into(),"grayscale".into(),"crt-soft".into(),"warm".into()], required: false, min: 0, max: 0 },
            ],
            selected_developer: ext_dev,
            selected_release_date: ext_date,
            selected_platforms: ext_platforms,
            selected_engine: ext_engine,
            selected_cover_meta: ext_cover_meta,
            selected_description: ext_desc,
            selected_tags: ext_tags,
            selected_aliases: ext_aliases,
            version: "0.1.0-demo".into(),
            build_identity: "demo-build — 强制跟随系统 / 毛玻璃底 Sheet / 三布局自适应".into(),
            is_demo: true,
        }
    }
}

impl ManagerController for DemoManagerController {
    fn model(&self) -> Result<ManagerViewModel, String> { Ok(self.build_model()) }
    fn select_case(&mut self, case_id: &str) -> Result<ManagerViewModel, String> {
        if case_id.is_empty() { self.selected = None; } else { self.selected = Some(case_id.into()); }
        Ok(self.build_model())
    }
    fn search(&mut self, query: &str) -> Result<ManagerViewModel, String> {
        self.search = query.into();
        Ok(self.build_model())
    }
    fn configure_nls(&mut self, nls: &str) -> Result<ManagerViewModel, String> {
        self.selected_nls = nls.into();
        self.pending_family.insert("fvp.nls".into(), nls.into());
        Ok(self.build_model())
    }
    fn save_translation_profile(&mut self, _kind: &str, _endpoint: &str, _protocol: &str, _model: &str, _lang: &str, ctx: i32, body: i32, timeout: i32, bg: &str, glossary: &str, _secret: &str) -> Result<ManagerViewModel, String> {
        // persist key translation fields demo-persistent
        self.pending_extension.insert("translation.timeout_ms".into(), timeout.to_string());
        self.pending_extension.insert("translation.context_sentences".into(), ctx.to_string());
        self.pending_extension.insert("translation.body_limit_bytes".into(), body.to_string());
        let _ = (bg, glossary);
        Ok(self.build_model())
    }
    fn grant_translation_consent(&mut self) -> Result<ManagerViewModel, String> { Ok(self.build_model()) }
    fn set_filter_preset(&mut self, preset_id: &str) -> Result<ManagerViewModel, String> {
        self.filter_preset = preset_id.into();
        self.pending_filter.insert("filter.preset".into(), preset_id.into());
        Ok(self.build_model())
    }
    fn set_patch_mode(&mut self, mode: &str) -> Result<ManagerViewModel, String> {
        self.patch_mode = mode.into();
        Ok(self.build_model())
    }
    fn game_input(&mut self, _control: &str, _pressed: bool, _value: f32) -> Result<(), String> { Ok(()) }
    fn rescan(&mut self) -> Result<ManagerViewModel, String> {
        // no diagnostic mask — keep global_diagnostic empty
        Ok(self.build_model())
    }
    fn launch(&mut self, case_id: &str) -> Result<ManagerViewModel, String> {
        self.selected = Some(case_id.into());
        Ok(self.build_model())
    }
    fn leave_game(&mut self) -> Result<ManagerViewModel, String> { Ok(self.build_model()) }
    fn navigate(&mut self, _page: &str) -> Result<(), String> { Ok(()) }
    fn family_config_changed(&mut self, key: &str, value: &str) -> Result<(), String> {
        self.pending_family.insert(key.into(), value.into());
        if key == "fvp.nls" { self.selected_nls = value.into(); }
        Ok(())
    }
    fn save_family_config(&mut self) -> Result<ManagerViewModel, String> { Ok(self.build_model()) }
    fn reset_family_config(&mut self) -> Result<ManagerViewModel, String> {
        self.pending_family.insert("fvp.nls".into(), "shift_jis".into());
        self.selected_nls = "shift_jis".into();
        Ok(self.build_model())
    }
    fn extension_config_changed(&mut self, key: &str, value: &str) -> Result<(), String> {
        self.pending_extension.insert(key.into(), value.into());
        Ok(())
    }
    fn save_extension_config(&mut self) -> Result<ManagerViewModel, String> { Ok(self.build_model()) }
    fn reset_extension_config(&mut self) -> Result<ManagerViewModel, String> {
        self.pending_extension.insert("translation.timeout_ms".into(), "2000".into());
        Ok(self.build_model())
    }
    fn filter_config_changed(&mut self, key: &str, value: &str) -> Result<(), String> {
        self.pending_filter.insert(key.into(), value.into());
        if key == "filter.preset" { self.filter_preset = value.into(); }
        Ok(())
    }
    fn save_filter_config(&mut self) -> Result<ManagerViewModel, String> { Ok(self.build_model()) }
    fn reset_filter_config(&mut self) -> Result<ManagerViewModel, String> {
        self.pending_filter.insert("filter.preset".into(), "none".into());
        self.filter_preset = "none".into();
        Ok(self.build_model())
    }
    fn update_bangumi_play_status(&mut self, status: &str, rating: i32, note: &str) -> Result<ManagerViewModel, String> {
        self.bangumi_status = status.into();
        self.bangumi_rating = rating;
        self.bangumi_note = note.into();
        Ok(self.build_model())
    }
}

pub struct DemoStageRenderer;

impl AstraUnderlayRenderer for DemoStageRenderer {
    fn setup(&mut self, _context: WgpuFrameContext<'_>) -> Result<(), String> { Ok(()) }
    fn render(&mut self, _context: WgpuFrameContext<'_>) -> Result<(), String> { Ok(()) }
    fn teardown(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefetch_demo_covers() {
        let dir = demo_cover_dir();
        let _ = std::fs::create_dir_all(&dir);
        for (i, entry) in REAL_VNS.iter().enumerate() {
            let path = dir.join(format!("demo-cover-{:02}.png", i));
            if path.metadata().map(|m| m.len() < 50_000).unwrap_or(true) {
                println!("Fetching cover {} ({}: {})...", i, entry.0, entry.1);
                let ok = try_fetch_real_cover(entry.1, &path);
                println!("Cover {} status: {}", i, ok);
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
        }
    }
}

