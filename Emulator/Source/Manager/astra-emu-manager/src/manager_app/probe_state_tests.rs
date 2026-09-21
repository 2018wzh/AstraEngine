use super::*;

use astra_emu_family_api::{
    FamilyDescriptor, FamilyOpen, FamilyProvider, FamilyResult, OpenRequest, ProbeReport,
    ProbeRequest,
};

struct MarkerProvider {
    descriptor: FamilyDescriptor,
    confidence_permyriad: u16,
}

impl FamilyProvider for MarkerProvider {
    fn descriptor(&self) -> FamilyResult<FamilyDescriptor> {
        Ok(self.descriptor.clone())
    }

    fn probe(&self, request: ProbeRequest) -> FamilyResult<Option<ProbeReport>> {
        let game_path = Path::new(request.game_path.as_str());
        if !game_path.join("match").is_file() {
            return Ok(None);
        }
        Ok(Some(ProbeReport {
            family_id: "fixture".into(),
            game_id: "fixture-game".into(),
            format: "fixture".into(),
            confidence_permyriad: self.confidence_permyriad,
        }))
    }

    fn open(&mut self, _request: OpenRequest) -> FamilyResult<FamilyOpen> {
        unreachable!("probe state tests do not open a family session")
    }
}

fn provider(plugin_id: &str, confidence_permyriad: u16) -> MarkerProvider {
    MarkerProvider {
        descriptor: FamilyDescriptor {
            configuration: Default::default(),
            family_id: "fixture".into(),
            plugin_id: plugin_id.into(),
            abi_fingerprint: astra_emu_family_api::FAMILY_ABI_FINGERPRINT.into(),
            version: "1.0.0".into(),
            capabilities: vec![astra_emu_family_api::FamilyCapability::CpuFrame].into(),
            supported_formats: vec!["fixture".into()].into(),
        },
        confidence_permyriad,
    }
}

fn controller(root: &Path) -> AstraEmuManagerController {
    AstraEmuManagerController::open_library(root.into(), FrameMailbox::new(), Vec::new()).unwrap()
}

fn game(root: &Path) -> PathBuf {
    let path = root.join("game");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("match"), b"match").unwrap();
    std::fs::canonicalize(path).unwrap()
}

fn scan_once(app: &mut AstraEmuManagerController, path: &Path) {
    app.scan_paths(&[path.to_owned()]).unwrap();
}

#[test]
fn selected_probe_is_cleared_on_no_match() {
    let root = tempfile::tempdir().unwrap();
    let game_path = game(root.path());
    let mut app = controller(root.path());
    app.registry
        .register_provider(provider("test-a", 9_000))
        .unwrap();
    scan_once(&mut app, &game_path);

    let id = path_id(&game_path);
    app.selected_case_id = Some(id.clone());
    assert!(app.candidates.contains_key(&id));
    assert_eq!(
        app.library.game(&id).unwrap().unwrap().family_id.as_deref(),
        Some("fixture")
    );

    std::fs::remove_file(game_path.join("match")).unwrap();
    scan_once(&mut app, &game_path);

    assert!(!app.candidates.contains_key(&id));
    assert!(!app.probe_choices.contains_key(&id));
    assert_eq!(app.library.game(&id).unwrap().unwrap().family_id, None);
    let model = app.model().unwrap();
    assert_eq!(model.games[0].family, "probe required");
    assert!(model.family_config_fields.is_empty());
    assert_eq!(
        app.launch(&id).unwrap_err(),
        "ASTRA_EMU_FAMILY_PROBE_REQUIRED"
    );
}

#[test]
fn ambiguous_probe_clears_old_candidate_and_rejects_old_choice_after_no_match() {
    let root = tempfile::tempdir().unwrap();
    let game_path = game(root.path());
    let mut app = controller(root.path());
    app.registry
        .register_provider(provider("test-a", 9_000))
        .unwrap();
    scan_once(&mut app, &game_path);

    let id = path_id(&game_path);
    app.selected_case_id = Some(id.clone());
    app.registry
        .register_provider(provider("test-b", 8_000))
        .unwrap();
    scan_once(&mut app, &game_path);

    assert!(!app.candidates.contains_key(&id));
    assert_eq!(app.probe_choices.get(&id).unwrap().len(), 2);
    assert_eq!(app.library.game(&id).unwrap().unwrap().family_id, None);
    assert_eq!(
        app.launch(&id).unwrap_err(),
        "ASTRA_EMU_FAMILY_PROVIDER_SELECTION_REQUIRED"
    );
    app.family_config_changed("family.plugin_id", "test-a")
        .unwrap();

    std::fs::remove_file(game_path.join("match")).unwrap();
    scan_once(&mut app, &game_path);

    assert!(!app.candidates.contains_key(&id));
    assert!(!app.probe_choices.contains_key(&id));
    assert_eq!(
        app.family_config_changed("family.plugin_id", "test-a")
            .unwrap_err(),
        "ASTRA_EMU_FAMILY_PROVIDER_SELECTION_NOT_REQUIRED"
    );
    assert_eq!(
        app.save_family_config().unwrap_err(),
        "ASTRA_EMU_FAMILY_PROBE_REQUIRED"
    );
}

#[test]
fn selected_probe_is_recreated_after_match_returns() {
    let root = tempfile::tempdir().unwrap();
    let game_path = game(root.path());
    let mut app = controller(root.path());
    app.registry
        .register_provider(provider("test-a", 9_000))
        .unwrap();
    scan_once(&mut app, &game_path);

    let id = path_id(&game_path);
    assert!(app.candidates.contains_key(&id));

    std::fs::remove_file(game_path.join("match")).unwrap();
    scan_once(&mut app, &game_path);
    assert!(!app.candidates.contains_key(&id));
    assert_eq!(app.library.game(&id).unwrap().unwrap().family_id, None);

    std::fs::write(game_path.join("match"), b"match").unwrap();
    scan_once(&mut app, &game_path);

    let candidate = app.candidates.get(&id).unwrap();
    assert_eq!(candidate.report.plugin_id, "test-a");
    assert_eq!(
        app.library.game(&id).unwrap().unwrap().family_id.as_deref(),
        Some("fixture")
    );
}
