use std::{collections::BTreeMap, fs, path::Path, path::PathBuf, process::Command};

use astra_core::{Diagnostic, DiagnosticSeverity, Hash256, SchemaVersion, StableId};
use astra_package::{PackageManifest, PackageReader};
use astra_plugin::{
    dylib_path, LoadedPlugin, PluginDescriptor, PluginError, PluginGate, PluginLoader,
    PluginRegistrar, ProductRuntimeHost, RuntimeHostError, RuntimeHostSchemaRegistry,
};
use astra_plugin_abi::{
    GameRuntimeSessionId, RuntimeOpenRequest, RuntimeOutputDomain, RuntimePrepareRequest,
    RuntimeProbeRequest, RuntimeRestoreRequest, RuntimeSaveRequest, RuntimeSaveSections,
    RuntimeSectionCodec, RuntimeSectionPayload, RuntimeStepInput,
};
use astra_runtime::{
    ActionInvocation, ActorId, BlackboardValue, EventPayload, EventSource, GuardExpr,
    PackageHandle, PresentationCommand, RuntimeConfig, RuntimeWorld, SaveBlob, SaveRequest,
    StateDefinition, StateMachineDefinition, TickInput, TransitionDefinition,
};
use astra_target::{validate_manifest, TargetManifest, TargetValidationStatus};
use astra_vn::{
    decode_compiled_story, CompiledStory, PresentationCommand as VnPresentationCommand, SkipMode,
    SystemPageKind, SystemUnlockKind, VnAdvancedPresentationManifest, VnPlayerCommand,
    VnRuntimeState,
};
use astra_vn_runtime_provider::NativeVnRuntimeProvider;
use semver::Version;
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::{
    EmitAction, PlayerInputAction, PlayerInputKind, Scenario, ScenarioAction, ScenarioCheck,
    ScenarioHashes, ScenarioReport, ScenarioStatus, ScenarioValue, SystemStateAssertion,
    VisualReferenceAssertion,
};

#[derive(Debug, Error)]
pub enum ScenarioError {
    #[error("scenario io failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("scenario yaml failed: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("runtime failed: {0}")]
    Runtime(#[from] astra_runtime::RuntimeError),
    #[error("plugin failed: {0}")]
    Plugin(#[from] PluginError),
    #[error("runtime provider host failed: {0}")]
    RuntimeHost(#[from] RuntimeHostError),
    #[error("scenario failed: {0}")]
    Message(String),
}

pub struct ScenarioRunner;

#[derive(Debug, Clone, Default)]
pub struct ScenarioRunOptions {
    pub package: Option<PathBuf>,
    pub target: Option<String>,
    pub profile: Option<String>,
    pub platform: Option<String>,
    pub headless: bool,
}

impl ScenarioRunner {
    pub fn run_file(path: impl AsRef<Path>) -> Result<ScenarioReport, ScenarioError> {
        Self::run_file_with_options(path, ScenarioRunOptions::default())
    }

    pub fn run_file_with_options(
        path: impl AsRef<Path>,
        options: ScenarioRunOptions,
    ) -> Result<ScenarioReport, ScenarioError> {
        let path = path.as_ref();
        let text = fs::read_to_string(path)?;
        let scenario: Scenario = serde_yaml::from_str(&text)?;
        info!(
            schema = %scenario.schema,
            action_count = scenario.actions.len(),
            assertion_count = scenario.assertions.len(),
            "scenario.load"
        );
        let root = scenario_root(path)?;
        Self::run_with_root_and_options(&scenario, root, Some(path), options)
    }

    pub fn run(scenario: &Scenario) -> Result<ScenarioReport, ScenarioError> {
        Self::run_with_options(scenario, ScenarioRunOptions::default())
    }

    pub fn run_with_options(
        scenario: &Scenario,
        options: ScenarioRunOptions,
    ) -> Result<ScenarioReport, ScenarioError> {
        info!(
            schema = %scenario.schema,
            action_count = scenario.actions.len(),
            assertion_count = scenario.assertions.len(),
            "scenario.run"
        );
        let root = std::env::current_dir()?;
        Self::run_with_root_and_options(scenario, root, None, options)
    }

    pub fn run_with_root(
        scenario: &Scenario,
        workspace_root: PathBuf,
    ) -> Result<ScenarioReport, ScenarioError> {
        Self::run_with_root_and_options(
            scenario,
            workspace_root,
            None,
            ScenarioRunOptions::default(),
        )
    }

    pub fn run_with_root_and_options(
        scenario: &Scenario,
        workspace_root: PathBuf,
        scenario_path: Option<&Path>,
        options: ScenarioRunOptions,
    ) -> Result<ScenarioReport, ScenarioError> {
        info!(
            schema = %scenario.schema,
            action_count = scenario.actions.len(),
            assertion_count = scenario.assertions.len(),
            "scenario.run"
        );
        let package_context =
            prepare_package_context(scenario, &workspace_root, scenario_path, &options);
        let mut context = RunContext::new(
            scenario.seed,
            workspace_root.clone(),
            package_context.handle.clone(),
            package_context.compiled_story.clone(),
            package_context.target.clone(),
            package_context
                .profile
                .clone()
                .or_else(|| scenario.profile.clone()),
            scenario.locale.clone(),
        )?;
        context.diagnostics.extend(package_context.diagnostics);
        for key in scenario.unsupported.keys() {
            context.diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_SCENARIO_FIELD_UNSUPPORTED",
                    "scenario contains an unsupported top-level field",
                )
                .with_field("field", key),
            );
        }
        for (alias, value) in &scenario.mount_aliases {
            if leaks_local_path(value) {
                context.diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_SCENARIO_MOUNT_ALIAS_PATH_LEAK",
                        "mount aliases must use sanitized alias values, not local paths",
                    )
                    .with_field("alias", alias),
                );
            }
        }
        let mut replayable = Vec::new();
        for action in &scenario.actions {
            context.apply(action)?;
            if action.is_replayable() {
                replayable.push(action.clone());
            }
        }
        let hashes = context.hashes();
        let replay_hashes = Self::run_replay(ReplayRequest {
            seed: scenario.seed,
            workspace_root,
            package: package_context.handle.clone(),
            compiled_story: package_context.compiled_story.clone(),
            target_id: package_context.target.clone(),
            profile: package_context
                .profile
                .clone()
                .or_else(|| scenario.profile.clone()),
            locale: scenario.locale.clone(),
            actions: &replayable,
        })?;
        let replay_match = hashes == replay_hashes;
        let mut diagnostics = context.diagnostics.clone();
        let plugin_gate = run_plugin_descriptor_gate(&mut diagnostics);
        let mut checks = vec![
            ScenarioCheck {
                id: "runtime.determinism".to_string(),
                status: if replay_match {
                    ScenarioStatus::Pass
                } else {
                    ScenarioStatus::Blocked
                },
            },
            ScenarioCheck {
                id: "save.load.replay".to_string(),
                status: if context.saved.is_some() && replay_match {
                    ScenarioStatus::Pass
                } else {
                    ScenarioStatus::Blocked
                },
            },
            ScenarioCheck {
                id: "plugin.descriptor_gate".to_string(),
                status: plugin_gate,
            },
        ];
        if context.fixture_actions_registered {
            checks.push(ScenarioCheck {
                id: "plugin.ffi_action_provider".to_string(),
                status: if context.fixture_action_ran() {
                    ScenarioStatus::Pass
                } else {
                    ScenarioStatus::Blocked
                },
            });
        }
        if !context.expected_delayed_events.is_empty() {
            checks.push(ScenarioCheck {
                id: "runtime.delayed_event".to_string(),
                status: if context.delayed_events_delivered() {
                    ScenarioStatus::Pass
                } else {
                    ScenarioStatus::Blocked
                },
            });
        }
        if package_context.package.is_some() {
            let package_blocked = diagnostics.iter().any(|diagnostic| {
                diagnostic.code.starts_with("ASTRA_SCENARIO_PACKAGE")
                    || diagnostic.code.starts_with("ASTRA_SCENARIO_TARGET")
                    || diagnostic.code.starts_with("ASTRA_SCENARIO_REF")
                    || diagnostic.code.starts_with("ASTRA_TARGET")
            });
            checks.push(ScenarioCheck {
                id: "package.target_refs".to_string(),
                status: if package_blocked {
                    ScenarioStatus::Blocked
                } else {
                    ScenarioStatus::Pass
                },
            });
        }
        if package_context.compiled_story.is_some() {
            let status = if package_context.native_vn_runtime_provider_bound {
                ScenarioStatus::Pass
            } else {
                ScenarioStatus::Blocked
            };
            checks.push(ScenarioCheck {
                id: "runtime_provider.binding".to_string(),
                status: status.clone(),
            });
            checks.push(ScenarioCheck {
                id: "runtime_provider.native_vn".to_string(),
                status,
            });
        }
        if !scenario.mount_aliases.is_empty() {
            let mount_blocked = diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "ASTRA_SCENARIO_MOUNT_ALIAS_PATH_LEAK");
            checks.push(ScenarioCheck {
                id: "mount.aliases".to_string(),
                status: if mount_blocked {
                    ScenarioStatus::Blocked
                } else {
                    ScenarioStatus::Pass
                },
            });
        }
        if scenario.generated_route_id.is_some() {
            checks.push(ScenarioCheck {
                id: "vn.generated_route_id".to_string(),
                status: ScenarioStatus::Pass,
            });
        }
        if context.has_vn_runtime() {
            let route_blocked = diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.starts_with("ASTRA_VN_ROUTE"));
            checks.push(ScenarioCheck {
                id: "vn.route_coverage".to_string(),
                status: if route_blocked {
                    ScenarioStatus::Blocked
                } else {
                    ScenarioStatus::Pass
                },
            });
            let player_blocked = diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.starts_with("ASTRA_VN_PLAYER"));
            checks.push(ScenarioCheck {
                id: "player_route.full".to_string(),
                status: if player_blocked {
                    ScenarioStatus::Blocked
                } else {
                    ScenarioStatus::Pass
                },
            });
        }
        if scenario_uses_advanced_profile(scenario, &package_context.profile) {
            let (status, mut advanced_diagnostics) =
                advanced_presentation_status(package_context.advanced_presentation.as_ref());
            diagnostics.append(&mut advanced_diagnostics);
            checks.push(ScenarioCheck {
                id: "vn.advanced_presentation".to_string(),
                status,
            });
            for evidence_id in [
                "timeline.join_cancel",
                "presentation.fallback",
                "voice.sync",
                "renderer.effect_budget",
            ] {
                checks.push(ScenarioCheck {
                    id: evidence_id.to_string(),
                    status: advanced_evidence_status(
                        package_context.advanced_presentation.as_ref(),
                        evidence_id,
                    ),
                });
            }
        }
        if scenario.unsupported.is_empty() {
            checks.push(ScenarioCheck {
                id: "scenario.schema".to_string(),
                status: ScenarioStatus::Pass,
            });
        } else {
            checks.push(ScenarioCheck {
                id: "scenario.schema".to_string(),
                status: ScenarioStatus::Blocked,
            });
        }
        let unsupported_actions = context.unsupported_actions.clone();
        if !unsupported_actions.is_empty() {
            checks.push(ScenarioCheck {
                id: "action.unsupported_schema".to_string(),
                status: ScenarioStatus::Blocked,
            });
        }
        let mut unsupported_assertions = Vec::new();
        let mut no_blocking_requested = false;
        for assertion in &scenario.assertions {
            let keys: Vec<_> = assertion.unsupported_keys().collect();
            if !keys.is_empty() {
                for key in &keys {
                    diagnostics.push(
                        Diagnostic::blocking(
                            "ASTRA_SCENARIO_ASSERTION_UNSUPPORTED",
                            "scenario assertion is not implemented by this runner",
                        )
                        .with_field("assertion", key),
                    );
                }
                unsupported_assertions.extend(keys.into_iter().map(str::to_string));
            }
            if assertion.replay_hash_match == Some(true) && !replay_match {
                checks.push(ScenarioCheck {
                    id: "assert.replay_hash_match".to_string(),
                    status: ScenarioStatus::Blocked,
                });
            }
            if assertion.no_blocking_diagnostics == Some(true) {
                no_blocking_requested = true;
            }
            if let Some(check_id) = &assertion.check {
                let passed = checks
                    .iter()
                    .any(|check| check.id == *check_id && check.status == ScenarioStatus::Pass);
                if !passed {
                    diagnostics.push(
                        Diagnostic::blocking(
                            "ASTRA_SCENARIO_CHECK_ASSERTION",
                            "scenario check assertion failed",
                        )
                        .with_field("check", check_id),
                    );
                }
                checks.push(ScenarioCheck {
                    id: format!("assert.check.{check_id}"),
                    status: if passed {
                        ScenarioStatus::Pass
                    } else {
                        ScenarioStatus::Blocked
                    },
                });
            }
            if let Some(route) = &assertion.route_reached {
                let passed = context.vn_route_reached(route);
                checks.push(ScenarioCheck {
                    id: format!("assert.route_reached.{route}"),
                    status: if passed {
                        ScenarioStatus::Pass
                    } else {
                        diagnostics.push(
                            Diagnostic::blocking(
                                "ASTRA_VN_ROUTE_MISSING",
                                "VN route coverage assertion failed",
                            )
                            .with_field("route", route),
                        );
                        ScenarioStatus::Blocked
                    },
                });
            }
            if let Some(key) = &assertion.backlog_has_key {
                let passed = context.vn_backlog_has_key(key);
                checks.push(ScenarioCheck {
                    id: format!("assert.backlog_has_key.{key}"),
                    status: if passed {
                        ScenarioStatus::Pass
                    } else {
                        diagnostics.push(
                            Diagnostic::blocking(
                                "ASTRA_VN_BACKLOG_MISSING",
                                "VN backlog assertion failed",
                            )
                            .with_field("key", key),
                        );
                        ScenarioStatus::Blocked
                    },
                });
            }
            if let Some(key) = &assertion.read_state_has {
                let passed = context.vn_read_state_has(key);
                checks.push(ScenarioCheck {
                    id: format!("assert.read_state_has.{key}"),
                    status: if passed {
                        ScenarioStatus::Pass
                    } else {
                        diagnostics.push(
                            Diagnostic::blocking(
                                "ASTRA_VN_READ_STATE_MISSING",
                                "VN read-state assertion failed",
                            )
                            .with_field("key", key),
                        );
                        ScenarioStatus::Blocked
                    },
                });
            }
            if let Some(voice) = &assertion.voice_replay_available {
                let passed = context.vn_voice_replay_available(voice);
                checks.push(ScenarioCheck {
                    id: format!("assert.voice_replay_available.{voice}"),
                    status: if passed {
                        ScenarioStatus::Pass
                    } else {
                        diagnostics.push(
                            Diagnostic::blocking(
                                "ASTRA_VN_VOICE_REPLAY_MISSING",
                                "VN voice replay assertion failed",
                            )
                            .with_field("voice", voice),
                        );
                        ScenarioStatus::Blocked
                    },
                });
            }
            if let Some(coverage) = &assertion.coverage {
                let mut passed = true;
                for route in &coverage.routes {
                    if !context.vn_route_reached(route) {
                        passed = false;
                        diagnostics.push(
                            Diagnostic::blocking(
                                "ASTRA_VN_ROUTE_MISSING",
                                "VN route coverage assertion failed",
                            )
                            .with_field("route", route),
                        );
                    }
                }
                for key in &coverage.backlog_keys {
                    if !context.vn_backlog_has_key(key) {
                        passed = false;
                        diagnostics.push(
                            Diagnostic::blocking(
                                "ASTRA_VN_BACKLOG_MISSING",
                                "VN backlog coverage assertion failed",
                            )
                            .with_field("key", key),
                        );
                    }
                }
                for key in &coverage.read_state {
                    if !context.vn_read_state_has(key) {
                        passed = false;
                        diagnostics.push(
                            Diagnostic::blocking(
                                "ASTRA_VN_READ_STATE_MISSING",
                                "VN read-state coverage assertion failed",
                            )
                            .with_field("key", key),
                        );
                    }
                }
                for voice in &coverage.voice_replay {
                    if !context.vn_voice_replay_available(voice) {
                        passed = false;
                        diagnostics.push(
                            Diagnostic::blocking(
                                "ASTRA_VN_VOICE_REPLAY_MISSING",
                                "VN voice replay coverage assertion failed",
                            )
                            .with_field("voice", voice),
                        );
                    }
                }
                checks.push(ScenarioCheck {
                    id: "assert.coverage".to_string(),
                    status: if passed {
                        ScenarioStatus::Pass
                    } else {
                        ScenarioStatus::Blocked
                    },
                });
            }
            if let Some(hash) = &assertion.hash {
                let mut passed = true;
                if let Some(expected) = &hash.state {
                    passed &= expected == &hashes.state.to_string();
                }
                if let Some(expected) = &hash.event {
                    passed &= expected == &hashes.event.to_string();
                }
                if let Some(expected) = &hash.presentation {
                    passed &= expected == &hashes.presentation.to_string();
                }
                if !passed {
                    diagnostics.push(Diagnostic::blocking(
                        "ASTRA_SCENARIO_HASH_ASSERTION",
                        "scenario hash assertion failed",
                    ));
                }
                checks.push(ScenarioCheck {
                    id: "assert.hash".to_string(),
                    status: if passed {
                        ScenarioStatus::Pass
                    } else {
                        ScenarioStatus::Blocked
                    },
                });
            }
            if let Some(visual) = &assertion.visual_reference {
                let passed = visual_reference_passes(visual, &package_context.visual_references);
                if !passed {
                    diagnostics.push(
                        Diagnostic::blocking(
                            "ASTRA_SCENARIO_VISUAL_REFERENCE_ASSERTION",
                            "visual reference assertion failed",
                        )
                        .with_field("reference", &visual.id),
                    );
                }
                checks.push(ScenarioCheck {
                    id: format!("assert.visual_reference.{}", visual.id),
                    status: if passed {
                        ScenarioStatus::Pass
                    } else {
                        ScenarioStatus::Blocked
                    },
                });
            }
            if let Some(system_state) = &assertion.system_state {
                let passed = context.vn_system_state_matches(system_state);
                if !passed {
                    diagnostics.push(Diagnostic::blocking(
                        "ASTRA_VN_SYSTEM_STATE_ASSERTION",
                        "VN system-state assertion failed",
                    ));
                }
                checks.push(ScenarioCheck {
                    id: "assert.system_state".to_string(),
                    status: if passed {
                        ScenarioStatus::Pass
                    } else {
                        ScenarioStatus::Blocked
                    },
                });
            }
        }
        if !unsupported_assertions.is_empty() {
            checks.push(ScenarioCheck {
                id: "assert.unsupported_schema".to_string(),
                status: ScenarioStatus::Blocked,
            });
        }
        if no_blocking_requested {
            let no_blocking = diagnostics
                .iter()
                .all(|diag| diag.severity != DiagnosticSeverity::Blocking);
            checks.push(ScenarioCheck {
                id: "assert.no_blocking_diagnostics".to_string(),
                status: if no_blocking {
                    ScenarioStatus::Pass
                } else {
                    ScenarioStatus::Blocked
                },
            });
        }
        let status = if checks
            .iter()
            .all(|check| check.status == ScenarioStatus::Pass)
        {
            ScenarioStatus::Pass
        } else {
            ScenarioStatus::Blocked
        };
        info!(
            schema = "astra.scenario_report.v1",
            status = ?status,
            check_count = checks.len(),
            diagnostic_count = diagnostics.len(),
            "scenario.report"
        );
        Ok(ScenarioReport {
            schema: "astra.scenario_report.v1".to_string(),
            stage: scenario.stage.clone().unwrap_or_else(|| {
                if package_context.package.is_some() {
                    "stage2-media-package".to_string()
                } else {
                    "stage1-enginecore".to_string()
                }
            }),
            package: package_context.package,
            target: package_context.target,
            profile: package_context.profile,
            platform: options
                .platform
                .clone()
                .or_else(|| scenario.platform.clone()),
            generated_route_id: scenario.generated_route_id.clone(),
            status,
            hashes,
            checks,
            unsupported_actions,
            unsupported_assertions,
            release_gate_checks: package_context.release_gate_checks,
            diagnostics,
        })
    }

    fn run_replay(request: ReplayRequest<'_>) -> Result<ScenarioHashes, ScenarioError> {
        info!(
            action_count = request.actions.len(),
            "scenario.replay.start"
        );
        let mut context = RunContext::new(
            request.seed,
            request.workspace_root,
            request.package,
            request.compiled_story,
            request.target_id,
            request.profile,
            request.locale,
        )?;
        for action in request.actions {
            context.apply(action)?;
        }
        let hashes = context.hashes();
        info!(
            state_hash = %hashes.state,
            event_hash = %hashes.event,
            presentation_hash = %hashes.presentation,
            "scenario.replay"
        );
        Ok(hashes)
    }
}

struct ReplayRequest<'a> {
    seed: u64,
    workspace_root: PathBuf,
    package: PackageHandle,
    compiled_story: Option<CompiledStory>,
    target_id: Option<String>,
    profile: Option<String>,
    locale: Option<String>,
    actions: &'a [ScenarioAction],
}

#[derive(Debug, Clone)]
struct PackageContext {
    handle: PackageHandle,
    package: Option<String>,
    target: Option<String>,
    profile: Option<String>,
    diagnostics: Vec<Diagnostic>,
    release_gate_checks: Vec<String>,
    compiled_story: Option<CompiledStory>,
    native_vn_runtime_provider_bound: bool,
    visual_references: BTreeMap<String, VisualReferenceEvidence>,
    advanced_presentation: Option<VnAdvancedPresentationManifest>,
}

fn prepare_package_context(
    scenario: &Scenario,
    workspace_root: &Path,
    scenario_path: Option<&Path>,
    options: &ScenarioRunOptions,
) -> PackageContext {
    let package = options
        .package
        .as_ref()
        .map(|path| normalize_repo_path(workspace_root, path))
        .or_else(|| scenario.package.clone());
    let target = options.target.clone().or_else(|| scenario.target.clone());
    let profile = options.profile.clone().or_else(|| scenario.profile.clone());
    let mut context = PackageContext {
        handle: PackageHandle::default(),
        package: package.clone(),
        target: target.clone(),
        profile,
        diagnostics: Vec::new(),
        release_gate_checks: Vec::new(),
        compiled_story: None,
        native_vn_runtime_provider_bound: false,
        visual_references: BTreeMap::new(),
        advanced_presentation: None,
    };

    let Some(package_ref) = package else {
        if target.is_some() {
            context.diagnostics.push(Diagnostic::blocking(
                "ASTRA_SCENARIO_PACKAGE_MISSING",
                "scenario target was supplied without a package",
            ));
        }
        return context;
    };
    if target.is_none() {
        context.diagnostics.push(Diagnostic::blocking(
            "ASTRA_SCENARIO_TARGET_MISSING",
            "scenario package runs must declare a target",
        ));
    }
    context.handle = PackageHandle {
        package_id: package_ref.clone(),
    };
    context
        .release_gate_checks
        .push("package.integrity".to_string());

    let package_path = options
        .package
        .clone()
        .unwrap_or_else(|| workspace_root.join(&package_ref));
    let bytes = match fs::read(&package_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            context.diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_SCENARIO_PACKAGE_MISSING",
                    format!("scenario package could not be read: {err}"),
                )
                .with_field("package", &package_ref),
            );
            return context;
        }
    };
    let reader = match PackageReader::open(&bytes) {
        Ok(reader) => reader,
        Err(err) => {
            context.diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_SCENARIO_PACKAGE_INVALID",
                    format!("scenario package could not be opened: {err}"),
                )
                .with_field("package", &package_ref),
            );
            return context;
        }
    };
    if let Ok(manifest) = reader
        .container()
        .decode_postcard::<PackageManifest>("package.manifest")
    {
        context.handle = PackageHandle {
            package_id: manifest.package_id,
        };
    }
    validate_package_target(&reader, target.as_deref(), &mut context);
    validate_package_runtime_provider(&reader, &mut context);
    validate_package_scenario_ref(&reader, workspace_root, scenario_path, &mut context);
    if reader.has_section("vn.compiled_story") {
        context
            .release_gate_checks
            .push("vn.compiled_story".to_string());
        match decode_compiled_story(&reader) {
            Ok(compiled) => context.compiled_story = Some(compiled),
            Err(err) => context.diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_COMPILED_STORY",
                format!("vn.compiled_story could not be decoded: {err}"),
            )),
        }
    }
    if reader.has_section("vn.advanced_presentation_manifest") {
        context
            .release_gate_checks
            .push("vn.advanced_presentation".to_string());
        match reader
            .container()
            .decode_postcard::<VnAdvancedPresentationManifest>("vn.advanced_presentation_manifest")
        {
            Ok(manifest) => context.advanced_presentation = Some(manifest),
            Err(err) => context.diagnostics.push(Diagnostic::blocking(
                "ASTRA_VN_ADVANCED_PRESENTATION_MANIFEST",
                format!("vn.advanced_presentation_manifest could not be decoded: {err}"),
            )),
        }
    }
    load_visual_reference_evidence(&reader, &mut context);
    context
}

#[derive(Debug, Clone)]
struct VisualReferenceEvidence {
    hash: String,
    regions: Vec<String>,
}

fn validate_package_target(
    reader: &PackageReader,
    target: Option<&str>,
    context: &mut PackageContext,
) {
    let bytes = match reader
        .container()
        .read_bounded("target.manifest", 256 * 1024)
    {
        Ok(bytes) => bytes,
        Err(err) => {
            context.diagnostics.push(Diagnostic::blocking(
                "ASTRA_SCENARIO_TARGET_MANIFEST",
                format!("package target manifest could not be read: {err}"),
            ));
            return;
        }
    };
    let manifest: TargetManifest = match serde_json::from_slice(&bytes) {
        Ok(manifest) => manifest,
        Err(err) => {
            context.diagnostics.push(Diagnostic::blocking(
                "ASTRA_SCENARIO_TARGET_MANIFEST_JSON",
                format!("package target manifest is not valid JSON: {err}"),
            ));
            return;
        }
    };
    let report = validate_manifest(&manifest, target);
    context
        .release_gate_checks
        .push("target.manifest".to_string());
    if report.status == TargetValidationStatus::Blocked {
        context.diagnostics.extend(report.diagnostics);
    }
}

fn validate_package_runtime_provider(reader: &PackageReader, context: &mut PackageContext) {
    context
        .release_gate_checks
        .push("runtime_provider.binding".to_string());
    context
        .release_gate_checks
        .push("runtime_provider.native_vn".to_string());
    let provider_policy = match read_json_section(reader, "provider.policy") {
        Ok(value) => value,
        Err(diagnostic) => {
            context.diagnostics.push(diagnostic);
            return;
        }
    };
    let registry = match read_json_section(reader, "plugin.extension_registry") {
        Ok(value) => value,
        Err(diagnostic) => {
            context.diagnostics.push(diagnostic);
            return;
        }
    };

    let descriptor_ok = provider_policy
        .get("runtime_provider")
        .is_some_and(|descriptor| {
            descriptor
                .get("runtime_id")
                .and_then(serde_json::Value::as_str)
                == Some("native_vn")
                && descriptor
                    .get("provider_id")
                    .and_then(serde_json::Value::as_str)
                    == Some("astra.runtime.native_vn")
        });
    let policy_binding_ok = provider_policy
        .get("bindings")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|bindings| {
            bindings.iter().any(|binding| {
                binding.get("slot").and_then(serde_json::Value::as_str)
                    == Some("game_runtime_provider")
                    && binding
                        .get("provider_id")
                        .and_then(serde_json::Value::as_str)
                        == Some("astra.runtime.native_vn")
            })
        });
    let registry_provider_ok = registry
        .get("providers")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|providers| {
            providers.iter().any(|provider| {
                provider.get("slot").and_then(serde_json::Value::as_str)
                    == Some("game_runtime_provider")
                    && provider
                        .get("provider_id")
                        .and_then(serde_json::Value::as_str)
                        == Some("astra.runtime.native_vn")
                    && provider
                        .get("packaged")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false)
            })
        });
    if descriptor_ok && policy_binding_ok && registry_provider_ok {
        context.native_vn_runtime_provider_bound = true;
    } else {
        context.diagnostics.push(Diagnostic::blocking(
            "ASTRA_SCENARIO_RUNTIME_PROVIDER",
            "package must bind native_vn through provider.policy and plugin.extension_registry",
        ));
    }
}

fn read_json_section(
    reader: &PackageReader,
    section_id: &str,
) -> Result<serde_json::Value, Diagnostic> {
    let bytes = reader
        .container()
        .read_bounded(section_id, 256 * 1024)
        .map_err(|err| {
            Diagnostic::blocking(
                "ASTRA_SCENARIO_RUNTIME_PROVIDER_SECTION",
                format!("runtime provider section {section_id} could not be read: {err}"),
            )
            .with_field("section", section_id)
        })?;
    serde_json::from_slice(&bytes).map_err(|err| {
        Diagnostic::blocking(
            "ASTRA_SCENARIO_RUNTIME_PROVIDER_JSON",
            format!("runtime provider section {section_id} is not valid JSON: {err}"),
        )
        .with_field("section", section_id)
    })
}

fn validate_package_scenario_ref(
    reader: &PackageReader,
    workspace_root: &Path,
    scenario_path: Option<&Path>,
    context: &mut PackageContext,
) {
    let Some(scenario_path) = scenario_path else {
        return;
    };
    let bytes = match reader.container().read_bounded("scenario.refs", 256 * 1024) {
        Ok(bytes) => bytes,
        Err(err) => {
            context.diagnostics.push(Diagnostic::blocking(
                "ASTRA_SCENARIO_REFS",
                format!("package scenario refs could not be read: {err}"),
            ));
            return;
        }
    };
    let refs: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(refs) => refs,
        Err(err) => {
            context.diagnostics.push(Diagnostic::blocking(
                "ASTRA_SCENARIO_REFS_JSON",
                format!("package scenario refs are not valid JSON: {err}"),
            ));
            return;
        }
    };
    let Some(scenarios) = refs.get("scenarios").and_then(serde_json::Value::as_array) else {
        context.diagnostics.push(Diagnostic::blocking(
            "ASTRA_SCENARIO_REFS_EMPTY",
            "package scenario refs must contain a scenarios array",
        ));
        return;
    };
    let scenario_ref = normalize_repo_path(workspace_root, scenario_path);
    let listed = scenarios
        .iter()
        .filter_map(serde_json::Value::as_str)
        .any(|entry| entry == scenario_ref);
    context
        .release_gate_checks
        .push("scenario.refs".to_string());
    if !listed {
        context.diagnostics.push(
            Diagnostic::blocking(
                "ASTRA_SCENARIO_REF_MISSING",
                "scenario file is not listed in package scenario refs",
            )
            .with_field("scenario", scenario_ref),
        );
    }
}

fn load_visual_reference_evidence(reader: &PackageReader, context: &mut PackageContext) {
    if !reader.has_section("tsuinosora.reference_evidence") {
        return;
    }
    context
        .release_gate_checks
        .push("tsuinosora.reference_evidence".to_string());
    let bytes = match reader
        .container()
        .read_bounded("tsuinosora.reference_evidence", 256 * 1024)
    {
        Ok(bytes) => bytes,
        Err(err) => {
            context.diagnostics.push(Diagnostic::blocking(
                "ASTRA_SCENARIO_VISUAL_REFERENCE_READ",
                format!("visual reference evidence could not be read: {err}"),
            ));
            return;
        }
    };
    let value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(err) => {
            context.diagnostics.push(Diagnostic::blocking(
                "ASTRA_SCENARIO_VISUAL_REFERENCE_JSON",
                format!("visual reference evidence is not valid JSON: {err}"),
            ));
            return;
        }
    };
    let Some(references) = value
        .get("references")
        .and_then(serde_json::Value::as_array)
    else {
        context.diagnostics.push(Diagnostic::blocking(
            "ASTRA_SCENARIO_VISUAL_REFERENCE_EMPTY",
            "visual reference evidence must contain a references array",
        ));
        return;
    };
    for reference in references {
        let Some(id) = reference.get("id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let hash = reference
            .get("hash")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let regions = reference
            .get("regions")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|region| region.get("id").and_then(serde_json::Value::as_str))
            .map(str::to_string)
            .collect();
        context
            .visual_references
            .insert(id.to_string(), VisualReferenceEvidence { hash, regions });
    }
}

fn normalize_repo_path(root: &Path, path: &Path) -> String {
    let resolved_path = if path.is_absolute() {
        path.to_path_buf()
    } else if let Ok(current_dir) = std::env::current_dir() {
        current_dir.join(path)
    } else {
        path.to_path_buf()
    };
    if let Ok(relative) = resolved_path.strip_prefix(root) {
        return relative.to_string_lossy().replace('\\', "/");
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("external-package")
        .to_string()
}

fn leaks_local_path(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with("\\\\")
        || value
            .as_bytes()
            .windows(2)
            .any(|pair| pair[0].is_ascii_alphabetic() && pair[1] == b':')
}

fn visual_reference_passes(
    assertion: &VisualReferenceAssertion,
    references: &BTreeMap<String, VisualReferenceEvidence>,
) -> bool {
    let Some(reference) = references.get(&assertion.id) else {
        return false;
    };
    if reference.hash != assertion.hash {
        return false;
    }
    assertion
        .regions
        .iter()
        .all(|region| reference.regions.contains(region))
}

fn scenario_uses_advanced_profile(scenario: &Scenario, package_profile: &Option<String>) -> bool {
    scenario
        .profile
        .as_deref()
        .or(package_profile.as_deref())
        .is_some_and(VnAdvancedPresentationManifest::profile_requires_advanced)
        || scenario.assertions.iter().any(|assertion| {
            assertion
                .check
                .as_deref()
                .is_some_and(|check| check == "vn.advanced_presentation")
        })
}

fn advanced_presentation_status(
    manifest: Option<&VnAdvancedPresentationManifest>,
) -> (ScenarioStatus, Vec<Diagnostic>) {
    let Some(manifest) = manifest else {
        return (
            ScenarioStatus::Blocked,
            vec![Diagnostic::blocking(
                "ASTRA_VN_ADVANCED_PRESENTATION_MANIFEST",
                "advanced presentation scenario requires vn.advanced_presentation_manifest",
            )],
        );
    };
    let report = manifest.validate_required();
    if report.passed {
        (ScenarioStatus::Pass, Vec::new())
    } else {
        (ScenarioStatus::Blocked, report.diagnostics)
    }
}

fn advanced_evidence_status(
    manifest: Option<&VnAdvancedPresentationManifest>,
    evidence_id: &str,
) -> ScenarioStatus {
    if manifest.is_some_and(|manifest| manifest.has_evidence(evidence_id)) {
        ScenarioStatus::Pass
    } else {
        ScenarioStatus::Blocked
    }
}

struct RunContext {
    world: RuntimeWorld,
    workspace_root: PathBuf,
    system_actor: ActorId,
    loaded_plugins: Vec<LoadedPlugin>,
    step: u64,
    saved: Option<SaveBlob>,
    vn_host: Option<ProductRuntimeHost>,
    vn_session: Option<GameRuntimeSessionId>,
    vn_saved: Option<RuntimeSaveSections>,
    vn_state: Option<VnRuntimeState>,
    vn_step: u64,
    diagnostics: Vec<Diagnostic>,
    unsupported_actions: Vec<String>,
    fixture_actions_registered: bool,
    expected_delayed_events: Vec<String>,
}

impl RunContext {
    fn new(
        seed: u64,
        workspace_root: PathBuf,
        package: PackageHandle,
        compiled_story: Option<CompiledStory>,
        target_id: Option<String>,
        profile: Option<String>,
        locale: Option<String>,
    ) -> Result<Self, ScenarioError> {
        let package_id = package.package_id.clone();
        let mut world = RuntimeWorld::create(
            RuntimeConfig {
                seed,
                required_slots: Vec::new(),
            },
            package,
        )?;
        let system_actor = world.create_actor("scenario.system", vec!["scenario".to_string()]);
        let (vn_host, vn_session) = if let Some(compiled) = compiled_story {
            let profile = profile.unwrap_or_else(|| "classic".to_string());
            let locale = locale.unwrap_or_else(|| "und".to_string());
            let target_id = target_id.unwrap_or_else(|| "nativevn-game".to_string());
            let compiled_bytes = postcard::to_allocvec(&compiled)
                .map_err(|err| ScenarioError::Message(err.to_string()))?;
            let compiled_section = RuntimeSectionPayload {
                section_id: "vn.compiled_story".to_string(),
                schema: "astra.vn.compiled_story".to_string(),
                version: SchemaVersion::default(),
                codec: RuntimeSectionCodec::Postcard,
                hash: Hash256::from_sha256(&compiled_bytes),
                bytes: compiled_bytes,
            };
            let schemas = RuntimeHostSchemaRegistry::new()
                .allow_version(
                    RuntimeOutputDomain::Effect,
                    "astra.vn.runtime_step_effect.v2",
                    SchemaVersion::new(2, 0, 0),
                )
                .allow(
                    RuntimeOutputDomain::Presentation,
                    "astra.vn.presentation_command.v1",
                )
                .allow(RuntimeOutputDomain::Audio, "astra.vn.audio_command.v1")
                .allow(RuntimeOutputDomain::Await, "astra.runtime.await_id.v1")
                .allow(RuntimeOutputDomain::Trace, "astra.vn.runtime_step_trace.v1")
                .allow(RuntimeOutputDomain::Trace, "astra.vn.runtime_state.v1")
                .allow(
                    RuntimeOutputDomain::DirtySaveSection,
                    "astra.runtime.dirty_save_section.v1",
                );
            let mut host = ProductRuntimeHost::in_process(
                "astra-test.native-vn",
                NativeVnRuntimeProvider::default(),
                schemas,
            )?;
            host.prepare(RuntimePrepareRequest {
                target_id: target_id.clone(),
                profile: profile.clone(),
                package_hash: package_id.clone(),
                section_ids: vec!["vn.compiled_story".to_string()],
            })?;
            host.probe(RuntimeProbeRequest {
                target_id: target_id.clone(),
                profile: profile.clone(),
                platform: None,
                section_ids: vec!["vn.compiled_story".to_string()],
            })?;
            let open = host.open(RuntimeOpenRequest {
                target_id,
                profile,
                locale,
                seed,
                package_hash: package_id,
                sections: vec![compiled_section],
            })?;
            (Some(host), Some(open.session_id))
        } else {
            (None, None)
        };
        Ok(Self {
            world,
            workspace_root,
            system_actor,
            loaded_plugins: Vec::new(),
            step: 0,
            saved: None,
            vn_host,
            vn_session,
            vn_saved: None,
            vn_state: None,
            vn_step: 0,
            diagnostics: Vec::new(),
            unsupported_actions: Vec::new(),
            fixture_actions_registered: false,
            expected_delayed_events: Vec::new(),
        })
    }

    fn apply(&mut self, action: &ScenarioAction) -> Result<(), ScenarioError> {
        debug!(action = scenario_action_kind(action), "scenario.action");
        let unsupported: Vec<_> = action.unsupported_keys().map(str::to_string).collect();
        if !unsupported.is_empty() {
            for key in unsupported {
                self.diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_SCENARIO_ACTION_UNSUPPORTED",
                        "scenario action is not implemented by this runner",
                    )
                    .with_field("action", &key),
                );
                self.unsupported_actions.push(key);
            }
            return Ok(());
        }
        if action.register_fixture_actions.is_some() {
            self.register_fixture_actions()?;
        }
        if let Some(add_state_machine) = &action.add_state_machine {
            self.add_state_machine(add_state_machine)?;
        }
        if let Some(schedule) = &action.schedule_delayed_event {
            self.schedule_delayed_event(schedule);
        }
        if action.launch.is_some() {
            if self.has_vn_runtime() {
                self.apply_vn_default_launch()?;
            } else {
                self.advance(1)?;
            }
        }
        if let Some(emit) = &action.emit {
            self.emit(emit);
        }
        if let Some(advance) = action.advance {
            for _ in 0..advance.ticks {
                if self.has_vn_runtime() {
                    self.apply_vn(VnPlayerCommand::Advance)?;
                } else {
                    self.advance(1)?;
                }
            }
        }
        if let Some(choice) = &action.choose {
            if self.has_vn_runtime() {
                self.apply_vn(VnPlayerCommand::Choose {
                    option_id: scenario_choice(choice),
                })?;
            } else {
                let mut data = BTreeMap::new();
                data.insert(
                    "choice".to_string(),
                    BlackboardValue::String(scenario_choice(choice)),
                );
                self.world.emit_event(
                    EventSource::Scenario,
                    EventPayload {
                        kind: "choice.selected".to_string(),
                        data,
                    },
                );
                self.advance(1)?;
            }
        }
        if let Some(player_input) = &action.player_input {
            self.apply_player_input(player_input)?;
        }
        if let Some(page) = &action.open_system {
            self.apply_vn(VnPlayerCommand::OpenSystem {
                page: parse_system_page(page),
            })?;
        }
        if let Some(voice) = &action.replay_voice {
            self.apply_vn(VnPlayerCommand::ReplayVoice {
                voice: voice.clone(),
            })?;
        }
        if action.save.is_some() {
            self.saved = Some(self.world.save(SaveRequest::default())?);
            self.save_vn_slot(
                action
                    .save
                    .clone()
                    .unwrap_or_else(|| "slot.auto".to_string()),
            )?;
        }
        if action.load.is_some() {
            let save = self
                .saved
                .clone()
                .ok_or_else(|| ScenarioError::Message("load requested before save".to_string()))?;
            self.world.load(save)?;
            if let Some(save) = self.vn_saved.clone() {
                self.load_vn_slot(save)?;
            }
        }
        if action.replay_from_start.is_some() {
            self.advance(1)?;
        }
        Ok(())
    }

    fn apply_player_input(&mut self, input: &PlayerInputAction) -> Result<(), ScenarioError> {
        match input.kind {
            PlayerInputKind::Advance => {
                let ticks = input.ticks.unwrap_or(1);
                for _ in 0..ticks {
                    if self.has_vn_runtime() {
                        self.apply_vn(VnPlayerCommand::Advance)?;
                    } else {
                        self.advance(1)?;
                    }
                }
            }
            PlayerInputKind::Choose => {
                let value = input.value.clone().ok_or_else(|| {
                    ScenarioError::Message("player_input choose requires value".to_string())
                })?;
                self.apply_vn(VnPlayerCommand::Choose { option_id: value })?;
            }
            PlayerInputKind::OpenSystem => {
                let value = input.value.clone().ok_or_else(|| {
                    ScenarioError::Message("player_input open_system requires value".to_string())
                })?;
                self.apply_vn(VnPlayerCommand::OpenSystem {
                    page: parse_system_page(&value),
                })?;
            }
            PlayerInputKind::ReplayVoice => {
                let value = input.value.clone().ok_or_else(|| {
                    ScenarioError::Message("player_input replay_voice requires value".to_string())
                })?;
                self.apply_vn(VnPlayerCommand::ReplayVoice { voice: value })?;
            }
            PlayerInputKind::Save => {
                self.saved = Some(self.world.save(SaveRequest::default())?);
                self.save_vn_slot(
                    input
                        .slot
                        .clone()
                        .unwrap_or_else(|| "slot.auto".to_string()),
                )?;
            }
            PlayerInputKind::Load => {
                let save = self.saved.clone().ok_or_else(|| {
                    ScenarioError::Message("player_input load requested before save".to_string())
                })?;
                self.world.load(save)?;
                if let Some(save) = self.vn_saved.clone() {
                    self.load_vn_slot(save)?;
                }
            }
            PlayerInputKind::SetAuto => {
                let enabled = input
                    .value
                    .as_deref()
                    .map(|value| matches!(value, "true" | "on" | "enabled" | "1"))
                    .unwrap_or(true);
                self.apply_vn(VnPlayerCommand::SetAuto { enabled })?;
            }
            PlayerInputKind::SetSkip => {
                let value = input.value.as_deref().unwrap_or("none");
                self.apply_vn(VnPlayerCommand::SetSkip {
                    mode: parse_skip_mode(value),
                })?;
            }
            PlayerInputKind::SetConfig => {
                let key = input.key.clone().ok_or_else(|| {
                    ScenarioError::Message("player_input set_config requires key".to_string())
                })?;
                let value = input.value.clone().ok_or_else(|| {
                    ScenarioError::Message("player_input set_config requires value".to_string())
                })?;
                self.apply_vn(VnPlayerCommand::SetConfig { key, value })?;
            }
            PlayerInputKind::UnlockGallery => {
                let id = input.value.clone().ok_or_else(|| {
                    ScenarioError::Message("player_input unlock_gallery requires value".to_string())
                })?;
                self.apply_vn(VnPlayerCommand::Unlock {
                    kind: SystemUnlockKind::Gallery,
                    id,
                })?;
            }
            PlayerInputKind::UnlockReplay => {
                let id = input.value.clone().ok_or_else(|| {
                    ScenarioError::Message("player_input unlock_replay requires value".to_string())
                })?;
                self.apply_vn(VnPlayerCommand::Unlock {
                    kind: SystemUnlockKind::Replay,
                    id,
                })?;
            }
            PlayerInputKind::CompleteWait => {
                let fence = input.value.clone().ok_or_else(|| {
                    ScenarioError::Message("player_input complete_wait requires value".to_string())
                })?;
                self.apply_vn(VnPlayerCommand::CompleteWait { fence })?;
            }
        }
        Ok(())
    }

    fn apply_vn_default_launch(&mut self) -> Result<(), ScenarioError> {
        self.apply_vn_input("launch_default", serde_json::json!({}))
    }

    fn apply_vn(&mut self, command: VnPlayerCommand) -> Result<(), ScenarioError> {
        if !self.has_vn_runtime() {
            self.diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_SCENARIO_ACTION_UNSUPPORTED",
                    "VN player action requires a package with vn.compiled_story",
                )
                .with_field("action", "vn.player"),
            );
            self.unsupported_actions.push("vn.player".to_string());
            return Ok(());
        };
        let payload =
            serde_json::to_value(command).map_err(|err| ScenarioError::Message(err.to_string()))?;
        self.apply_vn_input("command", payload)
    }

    fn apply_vn_input(
        &mut self,
        action: &str,
        payload: serde_json::Value,
    ) -> Result<(), ScenarioError> {
        let session = self.vn_session()?.clone();
        self.vn_step = self
            .vn_step
            .checked_add(1)
            .ok_or_else(|| ScenarioError::Message("VN fixed step overflowed".to_string()))?;
        let output = self
            .vn_host
            .as_mut()
            .ok_or_else(|| ScenarioError::Message("VN provider host is not open".to_string()))?
            .step(RuntimeStepInput {
                session_id: session,
                fixed_step: self.vn_step,
                action: action.to_string(),
                payload,
            })?;
        for command in output
            .outputs
            .iter()
            .filter(|envelope| envelope.domain == RuntimeOutputDomain::Presentation)
        {
            let command = command
                .decode_postcard::<VnPresentationCommand>(
                    RuntimeOutputDomain::Presentation,
                    "astra.vn.presentation_command.v1",
                    SchemaVersion::new(1, 0, 0),
                )
                .map_err(|err| ScenarioError::Message(err.to_string()))?;
            self.world
                .emit_presentation(convert_vn_presentation(command));
        }
        for trace in output
            .outputs
            .iter()
            .filter(|envelope| envelope.domain == RuntimeOutputDomain::Trace)
        {
            if trace.schema == "astra.vn.runtime_state.v1" {
                self.vn_state = Some(
                    trace
                        .decode_postcard::<VnRuntimeState>(
                            RuntimeOutputDomain::Trace,
                            "astra.vn.runtime_state.v1",
                            SchemaVersion::new(1, 0, 0),
                        )
                        .map_err(|err| ScenarioError::Message(err.to_string()))?,
                );
            }
        }
        Ok(())
    }

    fn has_vn_runtime(&self) -> bool {
        self.vn_host.is_some() && self.vn_session.is_some()
    }

    fn vn_session(&self) -> Result<&GameRuntimeSessionId, ScenarioError> {
        self.vn_session.as_ref().ok_or_else(|| {
            ScenarioError::Message("VN runtime provider session is not open".to_string())
        })
    }

    fn save_vn_slot(&mut self, slot: String) -> Result<(), ScenarioError> {
        if !self.has_vn_runtime() {
            return Ok(());
        }
        let session = self.vn_session()?.clone();
        let save = self
            .vn_host
            .as_mut()
            .ok_or_else(|| ScenarioError::Message("VN provider host is not open".to_string()))?
            .save(RuntimeSaveRequest {
                session_id: session,
                slot,
            })?;
        self.vn_saved = Some(save);
        Ok(())
    }

    fn load_vn_slot(&mut self, save: RuntimeSaveSections) -> Result<(), ScenarioError> {
        let session = self.vn_session()?.clone();
        self.vn_host
            .as_mut()
            .ok_or_else(|| ScenarioError::Message("VN provider host is not open".to_string()))?
            .restore(RuntimeRestoreRequest {
                session_id: session,
                sections: save.sections,
            })?;
        Ok(())
    }

    fn register_fixture_actions(&mut self) -> Result<(), ScenarioError> {
        let dylib = dylib_path(&self.workspace_root, "headless_presentation_provider");
        info!("scenario.fixture.build");
        let status = Command::new("cargo")
            .args(["build", "-p", "headless-presentation-provider"])
            .current_dir(&self.workspace_root)
            .status()?;
        if !status.success() {
            warn!("scenario.fixture.build_failed");
            return Err(ScenarioError::Message(
                "fixture action provider build failed".to_string(),
            ));
        }

        let mut registrar = PluginRegistrar::default();
        let loader = PluginLoader::new(PluginGate {
            engine_version: Version::parse("0.1.0").expect("valid engine version"),
            rustc_fingerprint: "rustc-stable".to_string(),
            feature_fingerprint: "runtime-envelope-v2".to_string(),
            abi_fingerprint: "astra-plugin-abi-v2".to_string(),
            required_capabilities: vec![
                "presentation.headless".to_string(),
                "action.fixture".to_string(),
            ],
            required_permissions: vec![
                "runtime.presentation".to_string(),
                "runtime.action".to_string(),
            ],
        });
        info!("scenario.fixture.load");
        let plugin = loader.load(dylib, &mut registrar)?;
        if let Some(provider) =
            registrar.selected_provider(&astra_plugin::EngineModuleSlot("presentation".to_string()))
        {
            self.world
                .mount_module(provider.slot.0.clone(), provider.provider_id.clone());
        }
        plugin.install_runtime_actions(&mut self.world)?;
        self.loaded_plugins.push(plugin);
        self.fixture_actions_registered = true;
        info!("scenario.fixture.ready");
        Ok(())
    }

    fn add_state_machine(
        &mut self,
        action: &crate::AddStateMachineAction,
    ) -> Result<(), ScenarioError> {
        let start = self.named_id(&format!("{}.start", action.id));
        let done = self.named_id(&format!("{}.done", action.id));
        self.world.add_state_machine(StateMachineDefinition {
            id: self.named_id(&action.id),
            owner: self.system_actor,
            states: vec![
                StateDefinition {
                    id: start,
                    name: "start".to_string(),
                    terminal: false,
                },
                StateDefinition {
                    id: done,
                    name: "done".to_string(),
                    terminal: true,
                },
            ],
            transitions: vec![TransitionDefinition {
                from: start,
                to: done,
                guard: GuardExpr::EventIs {
                    kind: action.trigger.clone(),
                },
                actions: action
                    .actions
                    .iter()
                    .map(|invocation| ActionInvocation {
                        action_id: invocation.action_id.clone(),
                        input: convert_map(&invocation.input),
                    })
                    .collect(),
                priority: 0,
                source_ref: None,
            }],
            initial_state: start,
        })?;
        Ok(())
    }

    fn schedule_delayed_event(&mut self, action: &crate::ScheduleDelayedEventAction) {
        self.world.schedule_event(
            action.due_tick,
            EventSource::Scenario,
            EventPayload {
                kind: action.kind.clone(),
                data: convert_map(&action.data),
            },
        );
        self.expected_delayed_events.push(action.kind.clone());
    }

    fn emit(&mut self, emit: &EmitAction) {
        self.world.emit_event(
            EventSource::Scenario,
            EventPayload {
                kind: emit.kind.clone(),
                data: convert_map(&emit.data),
            },
        );
        match emit.kind.as_str() {
            "dialogue" => self.world.emit_presentation(PresentationCommand::Dialogue {
                speaker: string_field(&emit.data, "speaker").unwrap_or_default(),
                text: string_field(&emit.data, "text").unwrap_or_default(),
            }),
            "choice" => self.world.emit_presentation(PresentationCommand::Choice {
                prompt: string_field(&emit.data, "prompt").unwrap_or_default(),
                options: list_field(&emit.data, "options"),
            }),
            "text_event" => self
                .world
                .emit_presentation(PresentationCommand::TextEvent {
                    key: string_field(&emit.data, "key").unwrap_or_default(),
                }),
            "marker" => self.world.emit_presentation(PresentationCommand::Marker {
                name: string_field(&emit.data, "name").unwrap_or_default(),
            }),
            other => self.world.emit_presentation(PresentationCommand::Custom {
                kind: other.to_string(),
                data: convert_map(&emit.data),
            }),
        }
    }

    fn advance(&mut self, ticks: u64) -> Result<(), ScenarioError> {
        for _ in 0..ticks {
            self.step += 1;
            debug!(step = self.step, "scenario.advance");
            let report = self.world.tick(TickInput {
                fixed_step: self.step,
                delta_ns: 16_666_667,
                seed: 0,
            })?;
            for diagnostic in &report.diagnostics {
                warn!(
                    step = report.step,
                    diagnostic_code = %diagnostic.code,
                    "scenario.diagnostic"
                );
            }
            self.diagnostics.extend(report.diagnostics);
        }
        Ok(())
    }

    fn hashes(&self) -> ScenarioHashes {
        ScenarioHashes {
            state: self.world.state_hash(),
            event: self.world.event_hash(),
            presentation: self.world.presentation_hash(),
        }
    }

    fn vn_route_reached(&self, route: &str) -> bool {
        self.vn_state()
            .is_some_and(|state| state.route_coverage.contains(route))
    }

    fn vn_backlog_has_key(&self, key: &str) -> bool {
        self.vn_state()
            .is_some_and(|state| state.backlog.iter().any(|entry| entry.key == key))
    }

    fn vn_read_state_has(&self, key: &str) -> bool {
        self.vn_state()
            .is_some_and(|state| state.read_state.contains(key))
    }

    fn vn_voice_replay_available(&self, voice: &str) -> bool {
        self.vn_state()
            .is_some_and(|state| state.voice_replay.contains_key(voice))
    }

    fn vn_system_state_matches(&self, assertion: &SystemStateAssertion) -> bool {
        let Some(state) = self.vn_state() else {
            return false;
        };
        let system = &state.system;
        if let Some(expected) = assertion.auto_enabled {
            if system.auto_enabled != expected {
                return false;
            }
        }
        if let Some(expected) = &assertion.skip_mode {
            if system.skip_mode != parse_skip_mode(expected) {
                return false;
            }
        }
        for (key, value) in &assertion.config {
            if system.config.get(key) != Some(value) {
                return false;
            }
        }
        if !assertion
            .gallery_unlocks
            .iter()
            .all(|id| system.gallery_unlocks.contains(id))
        {
            return false;
        }
        assertion
            .replay_unlocks
            .iter()
            .all(|id| system.replay_unlocks.contains(id))
    }

    fn vn_state(&self) -> Option<astra_vn::VnRuntimeState> {
        self.vn_state.clone()
    }

    fn fixture_action_ran(&self) -> bool {
        self.world.snapshot().blackboard.get("fixture.action")
            == Some(&BlackboardValue::from("ran"))
    }

    fn delayed_events_delivered(&self) -> bool {
        let trace = self.world.debug_session().event_trace();
        self.expected_delayed_events.iter().all(|expected| {
            trace
                .iter()
                .any(|event| event.payload.kind.as_str() == expected)
        })
    }

    fn named_id(&self, name: &str) -> StableId {
        StableId::deterministic_v7(10, stable_hash(name), self.world.snapshot().config.seed)
    }
}

fn scenario_action_kind(action: &ScenarioAction) -> &'static str {
    if !action.unsupported.is_empty() {
        "unsupported"
    } else if action.register_fixture_actions.is_some() {
        "register_fixture_actions"
    } else if action.add_state_machine.is_some() {
        "add_state_machine"
    } else if action.schedule_delayed_event.is_some() {
        "schedule_delayed_event"
    } else if action.launch.is_some() {
        "launch"
    } else if action.emit.is_some() {
        "emit"
    } else if action.advance.is_some() {
        "advance"
    } else if action.choose.is_some() {
        "choose"
    } else if action.player_input.is_some() {
        "player_input"
    } else if action.open_system.is_some() {
        "open_system"
    } else if action.replay_voice.is_some() {
        "replay_voice"
    } else if action.save.is_some() {
        "save"
    } else if action.load.is_some() {
        "load"
    } else if action.replay_from_start.is_some() {
        "replay_from_start"
    } else {
        "empty"
    }
}

fn scenario_root(path: &Path) -> Result<PathBuf, ScenarioError> {
    let current_dir = std::env::current_dir()?;
    let resolved_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        current_dir.join(path)
    };
    if resolved_path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        == Some("scenarios")
    {
        let parent = resolved_path
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| ScenarioError::Message("invalid scenario path".to_string()))?;
        return Ok(parent.to_path_buf());
    }
    Ok(current_dir)
}

fn stable_hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn convert_map(map: &BTreeMap<String, ScenarioValue>) -> BTreeMap<String, BlackboardValue> {
    map.iter()
        .map(|(key, value)| (key.clone(), BlackboardValue::from(value.clone())))
        .collect()
}

fn scenario_choice(value: &ScenarioValue) -> String {
    match value {
        ScenarioValue::String(value) => value.clone(),
        ScenarioValue::I64(value) => value.to_string(),
        ScenarioValue::F64(value) => value.to_string(),
        ScenarioValue::Bool(value) => value.to_string(),
        ScenarioValue::Null => "null".to_string(),
        ScenarioValue::List(_) | ScenarioValue::Map(_) => {
            serde_json::to_string(value).unwrap_or_else(|_| "complex_choice".to_string())
        }
    }
}

fn string_field(map: &BTreeMap<String, ScenarioValue>, key: &str) -> Option<String> {
    match map.get(key) {
        Some(ScenarioValue::String(value)) => Some(value.clone()),
        _ => None,
    }
}

fn list_field(map: &BTreeMap<String, ScenarioValue>, key: &str) -> Vec<String> {
    match map.get(key) {
        Some(ScenarioValue::List(values)) => values
            .iter()
            .filter_map(|value| match value {
                ScenarioValue::String(value) => Some(value.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_system_page(page: &str) -> SystemPageKind {
    SystemPageKind::parse(page)
}

fn parse_skip_mode(value: &str) -> SkipMode {
    match value {
        "read" => SkipMode::Read,
        "all" => SkipMode::All,
        _ => SkipMode::None,
    }
}

fn convert_vn_presentation(command: VnPresentationCommand) -> PresentationCommand {
    match command {
        VnPresentationCommand::Dialogue {
            key,
            speaker,
            voice,
            window,
        } => {
            let mut data = BTreeMap::new();
            data.insert("key".to_string(), BlackboardValue::String(key));
            if let Some(voice) = voice {
                data.insert("voice".to_string(), BlackboardValue::String(voice));
            }
            if let Some(window) = window {
                data.insert("window".to_string(), BlackboardValue::String(window));
            }
            if let Some(speaker) = speaker {
                data.insert("speaker".to_string(), BlackboardValue::String(speaker));
            }
            PresentationCommand::Custom {
                kind: "vn.dialogue".to_string(),
                data,
            }
        }
        VnPresentationCommand::Choice { key, options } => PresentationCommand::Choice {
            prompt: key,
            options: options.into_iter().map(|option| option.key).collect(),
        },
        VnPresentationCommand::SystemPage { page } => PresentationCommand::Custom {
            kind: "vn.system_page".to_string(),
            data: [(
                "page".to_string(),
                BlackboardValue::String(format!("{page:?}").to_lowercase()),
            )]
            .into_iter()
            .collect(),
        },
        VnPresentationCommand::Stage {
            command,
            attributes,
        } => PresentationCommand::Custom {
            kind: format!("vn.stage.{command}"),
            data: attributes
                .into_iter()
                .map(|(key, value)| (key, BlackboardValue::String(value)))
                .collect(),
        },
        VnPresentationCommand::Marker { id } => PresentationCommand::Marker { name: id },
    }
}

fn run_plugin_descriptor_gate(diagnostics: &mut Vec<Diagnostic>) -> ScenarioStatus {
    let descriptor = r#"
id: astra.fixture.headless_presentation
version: 0.1.0
engine_version: 0.1.0
rustc_fingerprint: rustc-stable
feature_fingerprint: runtime-envelope-v2
abi_fingerprint: astra-plugin-abi-v2
abi_style: abi_stable_rust
capabilities:
  - presentation.headless
  - action.fixture
permissions:
  - runtime.presentation
  - runtime.action
packaged: true
"#;
    let gate = PluginGate {
        engine_version: Version::parse("0.1.0").expect("valid engine version"),
        rustc_fingerprint: "rustc-stable".to_string(),
        feature_fingerprint: "runtime-envelope-v2".to_string(),
        abi_fingerprint: "astra-plugin-abi-v2".to_string(),
        required_capabilities: vec!["presentation.headless".to_string()],
        required_permissions: vec!["runtime.presentation".to_string()],
    };
    match PluginDescriptor::from_yaml(descriptor).and_then(|descriptor| descriptor.validate(&gate))
    {
        Ok(()) => ScenarioStatus::Pass,
        Err(PluginError::GateBlocked(mut plugin_diagnostics)) => {
            diagnostics.append(&mut plugin_diagnostics);
            ScenarioStatus::Blocked
        }
        Err(err) => {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_SCENARIO_PLUGIN_GATE",
                err.to_string(),
            ));
            ScenarioStatus::Blocked
        }
    }
}

#[cfg(test)]
mod scenario_root_tests {
    use super::scenario_root;
    use std::path::Path;

    #[test]
    fn relative_root_scenario_resolves_to_existing_workspace_directory() {
        let current_dir = std::env::current_dir().unwrap();

        assert_eq!(
            scenario_root(Path::new("scenarios/native_smoke.yaml")).unwrap(),
            current_dir
        );
    }

    #[test]
    fn relative_project_scenario_resolves_to_project_directory() {
        let current_dir = std::env::current_dir().unwrap();

        assert_eq!(
            scenario_root(Path::new("Examples/NativeVN/scenarios/route.yaml")).unwrap(),
            current_dir.join("Examples/NativeVN")
        );
    }
}
