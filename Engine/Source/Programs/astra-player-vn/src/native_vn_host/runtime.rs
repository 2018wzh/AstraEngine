use super::*;
use astra_plugin_abi::{RuntimeRestoreReport, RuntimeShutdownReport};

pub(super) struct NativeVnRuntimeHost {
    runtime: Option<astra_vn_runtime_provider::NativeVnSession>,
    binding: ValidatedRuntimeProviderSelection,
    limits: RuntimeHostLimits,
    session: Option<GameRuntimeSessionId>,
    seed: u64,
    last_step: u64,
    next_mode: RuntimeStepMode,
    failed: bool,
    destroyed: bool,
}

impl NativeVnRuntimeHost {
    pub(super) fn new(
        binding: &ValidatedRuntimeProviderSelection,
        limits: RuntimeHostLimits,
    ) -> Result<Self, RuntimeHostError> {
        binding
            .validate_linked_descriptor(&NativeVnRuntimeProvider::descriptor())
            .map_err(|error| RuntimeHostError::new(error.code, error.message))?;
        Ok(Self {
            runtime: None,
            binding: binding.clone(),
            limits,
            session: None,
            seed: 0,
            last_step: 0,
            next_mode: RuntimeStepMode::Live,
            failed: false,
            destroyed: false,
        })
    }

    fn validate_request(&self, target: &str, profile: &str) -> Result<(), RuntimeHostError> {
        if self.destroyed || target != self.binding.target() || profile != self.binding.profile() {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_BINDING_CONTEXT",
                "NativeVN request does not match the active package binding",
            ));
        }
        Ok(())
    }

    fn validate_session(&self, session: &GameRuntimeSessionId) -> Result<(), RuntimeHostError> {
        if self.destroyed || self.runtime.is_none() || self.session.as_ref() != Some(session) {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SESSION",
                "NativeVN session is not open",
            ));
        }
        Ok(())
    }

    fn require_healthy(&self) -> Result<(), RuntimeHostError> {
        if self.failed {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SESSION_POISONED",
                "NativeVN execution failed; restore or close the session",
            ));
        }
        Ok(())
    }

    pub(super) fn open(
        &mut self,
        compiled: Arc<CompiledStory>,
        config: VnRunConfig,
        options: astra_vn_runtime_provider::NativeVnSessionConfig,
    ) -> Result<GameRuntimeSessionId, RuntimeHostError> {
        self.validate_request(&options.target_id, &config.profile)?;
        if self.session.is_some() {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_SESSION_DUPLICATE",
                "NativeVN host already owns a session",
            ));
        }
        let seed = options.seed;
        let runtime = astra_vn_runtime_provider::NativeVnSession::new(compiled, config, options)
            .map_err(|error| RuntimeHostError::new("ASTRA_RUNTIME_HOST_OPEN", error.to_string()))?;
        let session_id = runtime.id().clone();
        self.runtime = Some(runtime);
        self.session = Some(session_id.clone());
        self.seed = seed;
        self.last_step = 0;
        self.next_mode = RuntimeStepMode::Live;
        self.failed = false;
        Ok(session_id)
    }

    pub(super) fn step(
        &mut self,
        input: NativeVnStepInput,
    ) -> Result<NativeVnStepOutput, RuntimeHostError> {
        self.validate_session(&input.session_id)?;
        self.require_healthy()?;
        if self.last_step.checked_add(1) != Some(input.fixed_step)
            || input.delta_ns == 0
            || input.delta_ns > 1_000_000_000
            || input.session_seed != self.seed
            || input.mode != self.next_mode
        {
            self.failed = true;
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_STEP_ORDER",
                "NativeVN step violates session timing, seed or restore mode",
            ));
        }
        let step = input.fixed_step;
        self.failed = true;
        let result = self
            .runtime
            .as_mut()
            .expect("validated native session")
            .step(input)
            .map_err(|error| RuntimeHostError::new("ASTRA_RUNTIME_HOST_STEP", error.to_string()))
            .and_then(|output| {
                if Some(&output.session_id) != self.session.as_ref() {
                    return Err(RuntimeHostError::new(
                        "ASTRA_RUNTIME_HOST_OUTPUT_SESSION",
                        "NativeVN output belongs to another session",
                    ));
                }
                self.limits.validate_output_count(
                    output
                        .presentations
                        .len()
                        .saturating_add(output.audio.len())
                        .saturating_add(output.timeline.len()),
                )?;
                Ok(output)
            });
        self.failed = result.is_err();
        if result.is_ok() {
            self.last_step = step;
            self.next_mode = RuntimeStepMode::Live;
        }
        result
    }

    pub(super) fn save(
        &mut self,
        request: RuntimeSaveRequest,
    ) -> Result<RuntimeSaveSections, RuntimeHostError> {
        self.validate_session(&request.session_id)?;
        self.require_healthy()?;
        self.failed = true;
        let result = self
            .runtime
            .as_ref()
            .expect("validated native session")
            .save(request)
            .map_err(|error| RuntimeHostError::new("ASTRA_RUNTIME_HOST_SAVE", error.to_string()))
            .and_then(|report| {
                self.limits.validate_sections(&report.sections)?;
                Ok(report)
            });
        self.failed = result.is_err();
        result
    }

    pub(super) fn restore(
        &mut self,
        request: RuntimeRestoreRequest,
    ) -> Result<RuntimeRestoreReport, RuntimeHostError> {
        self.validate_session(&request.session_id)?;
        self.limits.validate_sections(&request.sections)?;
        let was_failed = self.failed;
        self.failed = true;
        let report = match self
            .runtime
            .as_mut()
            .expect("validated native session")
            .restore(request)
        {
            Ok(report) => report,
            Err(error) => {
                self.failed = was_failed;
                return Err(RuntimeHostError::new(
                    "ASTRA_RUNTIME_HOST_RESTORE",
                    error.to_string(),
                ));
            }
        };
        if Some(&report.session_id) != self.session.as_ref() || report.session_seed != self.seed {
            self.failed = true;
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_RESTORE_IDENTITY",
                "NativeVN restored session or seed does not match",
            ));
        }
        self.last_step = report.restored_fixed_step;
        self.next_mode = RuntimeStepMode::RestoreContinuation;
        self.failed = false;
        Ok(report)
    }

    pub(super) fn shutdown(&mut self) -> Result<RuntimeShutdownReport, RuntimeHostError> {
        let session = self.session.clone().ok_or_else(|| {
            RuntimeHostError::new("ASTRA_RUNTIME_HOST_SESSION", "NativeVN session is not open")
        })?;
        self.validate_session(&session)?;
        let report = self
            .runtime
            .take()
            .expect("validated native session")
            .close();
        self.session = None;
        Ok(report)
    }

    pub(super) fn destroy(&mut self) -> Result<(), RuntimeHostError> {
        if self.session.is_some() {
            return Err(RuntimeHostError::new(
                "ASTRA_RUNTIME_HOST_LIFECYCLE",
                "close NativeVN before destroying the host",
            ));
        }
        self.destroyed = true;
        Ok(())
    }

    pub(super) fn cleanup_after_failure(&mut self) -> Result<(), RuntimeHostError> {
        if self.session.is_some() {
            self.shutdown()?;
        }
        self.destroy()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> NativeVnHostCommandSource {
        let bytes = crate::test_native_package::product_package_with_request(
            "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:line.one speaker:hero #@id line.one\n", |_| {},
        );
        let package = astra_package::PackageReader::open(&bytes).unwrap();
        NativeVnHostCommandSource::from_package(
            &package,
            VnRunConfig::classic("en"),
            320,
            180,
            PlayerHostResourceId(1),
        )
        .unwrap()
    }

    fn step(host: &NativeVnRuntimeHost, n: u64, command: NativeVnStepCommand) -> NativeVnStepInput {
        NativeVnStepInput {
            session_id: host.session.clone().unwrap(),
            fixed_step: n,
            delta_ns: 16_666_667,
            session_seed: host.seed,
            mode: host.next_mode,
            command,
        }
    }

    fn save(host: &mut NativeVnRuntimeHost) -> RuntimeSaveSections {
        host.save(RuntimeSaveRequest {
            session_id: host.session.clone().unwrap(),
            slot: "slot.01".into(),
        })
        .unwrap()
    }

    #[test]
    fn typed_presentation_output_still_enforces_the_host_budget() {
        let mut source = source();
        let host = &mut source.host;
        host.limits = RuntimeHostLimits::new().with_bounds(0, 8 * 1024 * 1024);
        let error = host
            .step(step(host, 1, NativeVnStepCommand::LaunchDefault))
            .unwrap_err();
        assert!(error.to_string().contains("OUTPUT_COUNT"));
        assert!(host.failed);
        assert!(host
            .step(step(
                host,
                2,
                NativeVnStepCommand::Execute(VnPlayerCommand::Advance)
            ))
            .is_err());
        source.release_resources().unwrap();
        source.shutdown().unwrap();
    }

    #[test]
    fn player_preserves_explicit_launch_and_typed_system_values() {
        let mut source = source();
        source
            .command(VnPlayerCommand::Launch {
                story_id: "story.main".into(),
                state_id: "state.start".into(),
            })
            .unwrap();
        assert_eq!(
            source
                .runtime_state
                .as_ref()
                .unwrap()
                .cursor
                .as_ref()
                .unwrap()
                .state_id,
            "state.start"
        );
        for enabled in [false, true] {
            source
                .command(VnPlayerCommand::SetAudioEnabled { enabled })
                .unwrap();
            source
                .command(VnPlayerCommand::SetAuto { enabled })
                .unwrap();
            let state = source.runtime_state.as_ref().unwrap();
            assert_eq!(state.system.audio_enabled, enabled);
            assert_eq!(state.system.auto_enabled, enabled);
        }
        source
            .command(VnPlayerCommand::SetSkip {
                mode: astra_vn_core::SkipMode::Read,
            })
            .unwrap();
        assert_eq!(
            source.runtime_state.as_ref().unwrap().system.skip_mode,
            astra_vn_core::SkipMode::Read
        );
        source.release_resources().unwrap();
        source.shutdown().unwrap();
    }

    #[test]
    fn owned_runtime_enforces_binding_tick_failure_and_restore_continuation() {
        let mut source = source();
        let host = &mut source.host;
        assert!(host
            .validate_request("foreign", host.binding.profile())
            .is_err());
        assert!(host.destroy().is_err());
        host.step(step(host, 1, NativeVnStepCommand::LaunchDefault))
            .unwrap();
        let saved = save(host);
        assert!(host
            .step(step(
                host,
                1,
                NativeVnStepCommand::Execute(VnPlayerCommand::Advance)
            ))
            .unwrap_err()
            .to_string()
            .contains("STEP_ORDER"));
        assert!(host
            .save(RuntimeSaveRequest {
                session_id: saved.session_id.clone(),
                slot: "slot.01".into()
            })
            .is_err());
        host.restore(RuntimeRestoreRequest {
            session_id: saved.session_id,
            sections: saved.sections,
        })
        .unwrap();
        assert!(!host.failed);
        assert_eq!(host.next_mode, RuntimeStepMode::RestoreContinuation);
        host.step(step(
            host,
            2,
            NativeVnStepCommand::Execute(VnPlayerCommand::Advance),
        ))
        .unwrap();
        assert_eq!(host.next_mode, RuntimeStepMode::Live);
        source.release_resources().unwrap();
        source.host.shutdown().unwrap();
        source.host.destroy().unwrap();
        assert!(source
            .host
            .validate_request(source.host.binding.target(), source.host.binding.profile())
            .is_err());
    }

    #[test]
    fn invalid_restore_preserves_current_native_state_and_save_budget_failure_stops_execution() {
        let mut source = source();
        let host = &mut source.host;
        host.step(step(host, 1, NativeVnStepCommand::LaunchDefault))
            .unwrap();
        let saved = save(host);
        let mut invalid = saved.sections.clone();
        invalid[0].bytes[0] ^= 1;
        assert!(host
            .restore(RuntimeRestoreRequest {
                session_id: saved.session_id.clone(),
                sections: invalid
            })
            .is_err());
        assert!(!host.failed);
        assert_eq!(save(host), saved);
        host.limits = RuntimeHostLimits::new().with_bounds(256, 1);
        assert!(host
            .save(RuntimeSaveRequest {
                session_id: saved.session_id,
                slot: "slot.01".into()
            })
            .is_err());
        assert!(host.failed);
        assert!(host
            .step(step(
                host,
                2,
                NativeVnStepCommand::Execute(VnPlayerCommand::Advance)
            ))
            .is_err());
        source.release_resources().unwrap();
        source.shutdown().unwrap();
    }
}
