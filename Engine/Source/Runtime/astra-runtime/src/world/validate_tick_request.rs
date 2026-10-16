use super::*;

impl RuntimeWorld {
    pub(crate) fn validate_tick_request(&self, request: &TickRequest) -> Result<(), RuntimeError> {
        let input = request.timing;
        if request.mode != self.required_tick_mode {
            return Err(RuntimeError::diagnostic(
                Diagnostic::blocking(
                    "ASTRA_RUNTIME_TICK_MODE_INVALID",
                    "runtime tick mode does not match world lifecycle state",
                )
                .with_field("expected_mode", format!("{:?}", self.required_tick_mode))
                .with_field("actual_mode", format!("{:?}", request.mode)),
            ));
        }
        let mut previous_sequence = 0;
        for ingress in &request.ingress {
            if ingress.sequence == 0 || ingress.sequence <= previous_sequence {
                if self.integrity_mode == TickIntegrityMode::Evidence {
                    return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                        "ASTRA_RUNTIME_TICK_INGRESS_ORDER_INVALID",
                        "tick ingress sequence must be non-zero and strictly increasing",
                    )));
                } else {
                    warn!(
                        sequence = ingress.sequence,
                        previous = previous_sequence,
                        "runtime.tick.ingress_order_warn"
                    );
                }
            }
            previous_sequence = ingress.sequence;
        }
        let expected_step = self.step.checked_add(1).ok_or_else(|| {
            RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_TICK_STEP_OVERFLOW",
                "runtime fixed step cannot advance beyond u64::MAX",
            ))
        })?;
        if input.fixed_step != expected_step {
            return Err(RuntimeError::diagnostic(
                Diagnostic::blocking(
                    "ASTRA_RUNTIME_TICK_STEP_INVALID",
                    "runtime tick must advance by exactly one fixed step",
                )
                .with_field("current_step", self.step)
                .with_field("expected_step", expected_step)
                .with_field("actual_step", input.fixed_step),
            ));
        }
        if input.delta_ns == 0 || input.delta_ns > 1_000_000_000 {
            if self.integrity_mode == TickIntegrityMode::Evidence {
                return Err(RuntimeError::diagnostic(
                    Diagnostic::blocking(
                        "ASTRA_RUNTIME_TICK_DELTA_INVALID",
                        "runtime tick delta must be within the supported fixed-step range",
                    )
                    .with_field("delta_ns", input.delta_ns),
                ));
            } else {
                warn!(delta_ns = input.delta_ns, "runtime.tick.delta_warn");
            }
        }
        if input.seed != self.config.seed {
            return Err(RuntimeError::diagnostic(
                Diagnostic::blocking(
                    "ASTRA_RUNTIME_TICK_SEED_MISMATCH",
                    "runtime tick seed does not match the session seed",
                )
                .with_field("expected_seed", self.config.seed)
                .with_field("actual_seed", input.seed),
            ));
        }
        if let Some(slot) = self
            .config
            .required_slots
            .iter()
            .find(|slot| !self.mounted_modules.contains_key(*slot))
        {
            if self.integrity_mode == TickIntegrityMode::Evidence {
                return Err(RuntimeError::diagnostic(
                    Diagnostic::blocking(
                        "ASTRA_RUNTIME_MODULE_MISSING",
                        "runtime required module slot is not mounted",
                    )
                    .with_field("slot", slot),
                ));
            } else {
                warn!(slot = %slot, "runtime.tick.module_missing_warn");
            }
        }
        Ok(())
    }

    pub(super) fn tick_validated(&mut self, input: TickInput) -> Result<TickReport, RuntimeError> {
        debug!(
            step = input.fixed_step,
            delta_ns = input.delta_ns,
            required_slot_count = self.config.required_slots.len(),
            "runtime.tick.start"
        );
        self.step = input.fixed_step;
        self.id_source.set_step(input.fixed_step);
        self.diagnostics.clear();
        for token_id in self.tasks.cancelled() {
            self.cancel_await(token_id)?;
        }
        let await_drain = self.awaits.drain_ordered_results(input.fixed_step);
        for diagnostic in &await_drain.diagnostics {
            warn!(
                step = input.fixed_step,
                diagnostic_code = %diagnostic.code,
                "runtime.diagnostic"
            );
        }
        self.diagnostics.extend(await_drain.diagnostics);
        debug!(
            step = input.fixed_step,
            count = await_drain.results.len(),
            "runtime.await.drain"
        );
        for result in await_drain.results {
            self.tasks.finish(result.token_id, false);
            let id = EventId(self.next_id());
            self.events.push(RuntimeEvent {
                id,
                source: EventSource::AwaitResult,
                step: input.fixed_step,
                sequence: result.sequence,
                payload: result.payload,
            });
        }
        let delayed_events = self.delayed_events.drain_due(input.fixed_step);
        debug!(
            step = input.fixed_step,
            count = delayed_events.len(),
            "runtime.delayed_event.drain"
        );
        for event in delayed_events {
            self.events.push(event);
        }
        let ready = self.events.drain_ordered_for_step(input.fixed_step);
        debug!(
            step = input.fixed_step,
            count = ready.len(),
            "runtime.event.drain"
        );
        let machine_started = Instant::now();
        let output = self.machines.tick(
            input.fixed_step,
            &ready,
            &mut self.actors,
            &mut self.blackboard,
            &self.actions,
            &mut self.id_source,
            self.machine_worker_count,
            self.integrity_mode == TickIntegrityMode::Evidence,
        );
        let machine_ns = machine_started.elapsed().as_nanos() as u64;
        for diagnostic in &output.diagnostics {
            warn!(
                step = input.fixed_step,
                diagnostic_code = %diagnostic.code,
                "runtime.diagnostic"
            );
        }
        let output_diagnostic_count = output.diagnostics.len();
        self.diagnostics.extend(output.diagnostics);
        for id in output.delayed_cancellations {
            self.delayed_events.cancel(id);
        }
        for event in output.delayed_events {
            self.delayed_events.schedule(event);
        }
        for event in output.events {
            self.events.push(event);
        }
        for await_token in output.awaits {
            self.awaits
                .insert(await_token)
                .map_err(RuntimeError::diagnostic)?;
        }
        for command in output.presentation {
            self.emit_presentation(command)?;
        }
        crate::diagnostic_history::extend(&mut self.mutations, output.mutations);
        if let Some(diagnostic) = self.diagnostics.iter().find(|diagnostic| {
            matches!(
                diagnostic.severity,
                astra_core::DiagnosticSeverity::Blocking | astra_core::DiagnosticSeverity::Error
            )
        }) {
            return Err(RuntimeError::diagnostic(diagnostic.clone()));
        }
        let report = TickReport {
            step: input.fixed_step,
            integrity_mode: self.integrity_mode,
            diagnostics: self.diagnostics.clone(),
        };
        trace!(
            event = "runtime.tick.performance",
            step = report.step,
            diagnostic_count = report.diagnostics.len(),
            output_diagnostic_count,
            machine_ns,
            "measured RuntimeWorld tick phases"
        );
        Ok(report)
    }

    pub fn save(&self, request: SaveRequest) -> Result<SaveBlob, RuntimeError> {
        self.ensure_active()?;
        debug!(
            minimum_supported_version = ?request.minimum_supported_version,
            step = self.step,
            "runtime.save"
        );
        crate::save::write_runtime_save(self.snapshot()?, request)
    }

    pub fn load(&mut self, save: SaveBlob) -> Result<LoadReport, RuntimeError> {
        debug!("runtime.load");
        self.load_with_registry(save, &SchemaMigrationRegistry::default())
    }

    pub fn load_with_registry(
        &mut self,
        save: SaveBlob,
        registry: &SchemaMigrationRegistry,
    ) -> Result<LoadReport, RuntimeError> {
        self.load_with_validation(save, registry, |_| Ok(()))
            .map(|(report, ())| report)
    }

    /// Validate and prepare decoded state before replacing the live world.
    /// A rejected snapshot leaves the world, including its failure status, unchanged.
    pub fn load_with_validation<T>(
        &mut self,
        save: SaveBlob,
        registry: &SchemaMigrationRegistry,
        validate: impl FnOnce(&mut RuntimeSnapshot) -> Result<T, RuntimeError>,
    ) -> Result<(LoadReport, T), RuntimeError> {
        debug!("runtime.load.with_validation");
        let mut snapshot = crate::save::read_runtime_save(&save, registry)?;
        snapshot
            .awaits
            .validate()
            .map_err(RuntimeError::diagnostic)?;
        let validated = validate(&mut snapshot)?;
        snapshot
            .awaits
            .validate()
            .map_err(RuntimeError::diagnostic)?;
        self.restore_snapshot(snapshot);
        self.required_tick_mode = TickMode::RestoreContinuation;
        let report = LoadReport {
            step: self.step,
            seed: self.config.seed,
        };
        info!(
            event = "runtime.load",
            step = report.step,
            seed = report.seed,
            "restored runtime world"
        );
        Ok((report, validated))
    }

    pub fn restore_snapshot(&mut self, snapshot: RuntimeSnapshot) {
        self.tasks = crate::task_scope::TaskRuntime::default();
        self.failed = false;
        self.config = snapshot.config;
        self.package = snapshot.package;
        self.id_source = snapshot.id_source;
        self.actors = snapshot.actors;
        self.blackboard = snapshot.blackboard;
        self.machines = snapshot.machines;
        self.awaits = snapshot.awaits;
        self.delayed_events = snapshot.delayed_events;
        self.events = snapshot.events;
        self.presentation.clear();
        self.presentation_sequence = snapshot.presentation_sequence;
        self.mutations.clear();
        self.mounted_modules = snapshot.mounted_modules;
        self.integrity_mode = snapshot.integrity_mode;
        self.step = snapshot.step;
    }

    pub fn debug_session(&self) -> RuntimeDebugSession<'_> {
        RuntimeDebugSession { world: self }
    }

    pub fn snapshot(&self) -> Result<RuntimeSnapshot, RuntimeError> {
        self.ensure_active()?;
        Ok(RuntimeSnapshot {
            config: self.config.clone(),
            package: self.package.clone(),
            id_source: self.id_source.clone(),
            actors: self.actors.clone(),
            blackboard: self.blackboard.clone(),
            machines: self.machines.snapshot(),
            awaits: self.awaits.clone(),
            delayed_events: self.delayed_events.clone(),
            events: self.events.snapshot(),
            presentation_sequence: self.presentation_sequence,
            mounted_modules: self.mounted_modules.clone(),
            integrity_mode: self.integrity_mode,
            step: self.step,
        })
    }

    pub(super) fn next_id(&mut self) -> StableId {
        self.id_source.next_id()
    }
}

pub(super) fn presentation_kind(command: &PresentationCommand) -> &str {
    match command {
        PresentationCommand::Dialogue { .. } => "dialogue",
        PresentationCommand::Choice { .. } => "choice",
        PresentationCommand::TextEvent { .. } => "text_event",
        PresentationCommand::Marker { .. } => "marker",
        PresentationCommand::Custom { .. } => "custom",
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LoadReport {
    pub step: u64,
    pub seed: u64,
}

pub struct RuntimeDebugSession<'a> {
    pub(super) world: &'a RuntimeWorld,
}

impl RuntimeDebugSession<'_> {
    pub fn blackboard(&self) -> BTreeMap<String, BlackboardValue> {
        self.world.blackboard.values().clone()
    }

    pub fn actors(&self) -> Vec<ActorSnapshot> {
        self.world.actors.actor_snapshots()
    }

    pub fn components(&self, actor: ActorId) -> Vec<ComponentSnapshot> {
        self.world.actors.component_snapshots(actor)
    }

    pub fn state_machines(&self, actor: ActorId) -> Vec<StateMachineSnapshot> {
        self.world.machines.snapshots(actor)
    }

    pub fn event_trace(&self) -> Vec<RuntimeEvent> {
        self.world.events.trace().iter().cloned().collect()
    }

    pub fn presentation_trace(&self) -> Vec<PresentationRecord> {
        self.world.presentation.iter().cloned().collect()
    }

    pub fn mutation_trace(&self) -> Vec<RuntimeMutationRecord> {
        self.world.mutations.iter().cloned().collect()
    }

    pub fn action_trace(&self) -> Vec<crate::ActionTrace> {
        self.world.machines.trace().iter().cloned().collect()
    }
}
