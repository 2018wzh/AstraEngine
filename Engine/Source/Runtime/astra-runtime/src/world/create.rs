use super::*;

impl RuntimeWorld {
    pub fn create(config: RuntimeConfig) -> Result<Self, RuntimeError> {
        let integrity_mode = if cfg!(debug_assertions) {
            TickIntegrityMode::Evidence
        } else {
            TickIntegrityMode::Shipping
        };
        Self::create_with_integrity(config, integrity_mode)
    }

    pub fn create_with_integrity(
        config: RuntimeConfig,
        integrity_mode: TickIntegrityMode,
    ) -> Result<Self, RuntimeError> {
        let mut actions = ActionRegistry::default();
        actions.register(SetBlackboardAction)?;
        actions.register(EmitEventAction)?;
        actions.register(CreateAwaitAction)?;
        actions.register(PresentationAction)?;
        info!(
            seed = config.seed,
            required_slot_count = config.required_slots.len(),
            integrity_mode = ?integrity_mode,
            default_action_count = 4,
            "runtime.create"
        );
        Ok(Self {
            tasks: crate::task_scope::TaskRuntime::default(),
            failed: false,
            id_source: StableIdGenerator::new(config.seed),
            config,
            package: None,
            actors: ActorStore::default(),
            blackboard: Blackboard::default(),
            events: EventQueue::default(),
            awaits: AwaitQueue::default(),
            delayed_events: DelayedEventQueue::default(),
            machines: StateMachineStore::default(),
            actions,
            presentation: VecDeque::new(),
            presentation_sequence: 0,
            mutations: VecDeque::new(),
            diagnostics: Vec::new(),
            mounted_modules: BTreeMap::new(),
            step: 0,
            required_tick_mode: TickMode::Live,
            integrity_mode,
            machine_worker_count: 1,
        })
    }

    pub fn seed(&self) -> u64 {
        self.config.seed
    }

    pub fn tick_integrity_mode(&self) -> TickIntegrityMode {
        self.integrity_mode
    }

    pub fn set_machine_worker_count(&mut self, worker_count: usize) -> Result<(), RuntimeError> {
        self.ensure_active()?;
        if !(1..=8).contains(&worker_count) {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_WORKER_COUNT",
                "machine worker count must be within 1..=8",
            )));
        }
        if self.step != 0 {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_WORKER_COUNT_LIFECYCLE",
                "machine worker count must be configured before the first fixed tick",
            )));
        }
        self.machine_worker_count = worker_count;
        Ok(())
    }

    /// Attach product identity only for hosts using packaged module bindings.
    /// Standalone worlds use the same state, tick and save implementation without it.
    pub fn with_package(mut self, package: PackageHandle) -> Result<Self, RuntimeError> {
        self.ensure_active()?;
        if self.step != 0 || self.package.is_some() || !self.mounted_modules.is_empty() {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_PACKAGE_LIFECYCLE",
                "package identity must be attached once before ticking or mounting modules",
            )));
        }
        if [
            &package.package_id,
            &package.target,
            &package.profile,
            &package.engine_version,
            &package.rustc_fingerprint,
            &package.feature_fingerprint,
            &package.abi_fingerprint,
        ]
        .iter()
        .any(|value| value.is_empty() || value.len() > 256 || value.chars().any(char::is_control))
        {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_PACKAGE_IDENTITY",
                "package identity fields are invalid",
            )));
        }
        self.package = Some(package);
        Ok(self)
    }

    pub fn package_id(&self) -> Option<&str> {
        self.package
            .as_ref()
            .map(|package| package.package_id.as_str())
    }

    pub fn package_handle(&self) -> Option<&PackageHandle> {
        self.package.as_ref()
    }

    pub fn mount_module(
        &mut self,
        slot: EngineModuleSlot,
        binding: ValidatedModuleBinding,
    ) -> Result<(), RuntimeError> {
        self.ensure_active()?;
        if binding.slot != slot {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_MODULE_SLOT_MISMATCH",
                "module binding token does not match the requested slot",
            )));
        }
        let package = self.package.as_ref().ok_or_else(|| {
            RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_MODULE_PACKAGE_REQUIRED",
                "packaged module bindings require a host-supplied package identity",
            ))
        })?;
        let expected_context = ModuleBindingContext {
            package_id: package.package_id.clone(),
            target: package.target.clone(),
            profile: package.profile.clone(),
            engine_version: package.engine_version.clone(),
            rustc_fingerprint: package.rustc_fingerprint.clone(),
            feature_fingerprint: package.feature_fingerprint.clone(),
            abi_fingerprint: package.abi_fingerprint.clone(),
        };
        let actual_context = ModuleBindingContext {
            package_id: binding.snapshot.package_id.clone(),
            target: binding.snapshot.target.clone(),
            profile: binding.snapshot.profile.clone(),
            engine_version: binding.snapshot.engine_version.clone(),
            rustc_fingerprint: binding.snapshot.rustc_fingerprint.clone(),
            feature_fingerprint: binding.snapshot.feature_fingerprint.clone(),
            abi_fingerprint: binding.snapshot.abi_fingerprint.clone(),
        };
        if actual_context != expected_context {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_MODULE_CONTEXT_MISMATCH",
                "module binding token does not match package target/profile/fingerprint context",
            )));
        }
        if self.mounted_modules.contains_key(&slot.0) {
            return Err(RuntimeError::diagnostic(
                Diagnostic::blocking(
                    "ASTRA_RUNTIME_MODULE_SLOT_OCCUPIED",
                    "runtime module slot is already mounted",
                )
                .with_field("slot", &slot.0),
            ));
        }
        info!(slot = %slot.0, provider_id = %binding.snapshot.provider_id, "runtime.module.mount");
        self.mounted_modules.insert(slot.0, binding.snapshot);
        Ok(())
    }

    pub fn register_action<A: RuntimeAction + 'static>(
        &mut self,
        provider_id: impl Into<String>,
        action: A,
    ) -> Result<(), RuntimeError> {
        self.ensure_active()?;
        let provider_id = provider_id.into();
        let action_id = action.descriptor().id;
        info!(
            provider_id = %provider_id,
            action_id = %action_id,
            "runtime.action.register"
        );
        self.actions.register_with_provider(provider_id, action)
    }

    pub fn unregister_action_provider(&mut self, provider_id: &str) {
        info!(provider_id, "runtime.action.unregister_provider");
        self.actions.unregister_provider(provider_id);
    }

    pub fn create_actor(
        &mut self,
        name: impl Into<String>,
        tags: Vec<String>,
    ) -> Result<ActorId, RuntimeError> {
        self.ensure_active()?;
        let actor_id = ActorId(self.next_id());
        debug!(?actor_id, tag_count = tags.len(), "runtime.actor.create");
        self.actors.insert_actor(ActorRecord {
            actor_id,
            name: name.into(),
            tags,
            components: Vec::new(),
        });
        Ok(actor_id)
    }

    pub fn attach_component<T>(
        &mut self,
        actor_id: ActorId,
        schema: impl Into<SchemaId>,
        data: &T,
    ) -> Result<ComponentId, RuntimeError>
    where
        T: Serialize + Clone + std::fmt::Debug + Send + Sync + 'static,
    {
        self.ensure_active()?;
        let component_id = ComponentId(self.next_id());
        let schema = schema.into();
        let payload =
            RuntimeComponentPayload::typed(schema.clone(), SchemaVersion::default(), data.clone());
        let attached = self.actors.attach_component(ComponentRecord {
            component_id,
            actor_id,
            payload,
        });
        if attached {
            debug!(
                ?actor_id,
                ?component_id,
                schema = %schema,
                "runtime.component.attach"
            );
            Ok(component_id)
        } else {
            let diagnostic = Diagnostic::blocking(
                "ASTRA_RUNTIME_ACTOR_MISSING",
                format!("cannot attach component to missing actor {actor_id:?}"),
            );
            warn!(
                ?actor_id,
                diagnostic_code = %diagnostic.code,
                "runtime.diagnostic"
            );
            Err(RuntimeError::diagnostic(diagnostic))
        }
    }

    pub fn read_component<T>(&self, component_id: ComponentId) -> Result<T, RuntimeError>
    where
        T: Serialize + DeserializeOwned + Clone + std::fmt::Debug + Send + Sync + 'static,
    {
        let component = self.actors.component(component_id).ok_or_else(|| {
            RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_COMPONENT_MISSING",
                "runtime component does not exist",
            ))
        })?;
        component.payload.decode()
    }

    pub fn replace_component<T>(
        &mut self,
        component_id: ComponentId,
        data: &T,
    ) -> Result<(), RuntimeError>
    where
        T: Serialize + Clone + std::fmt::Debug + Send + Sync + 'static,
    {
        self.ensure_active()?;
        let component = self.actors.component_mut(component_id).ok_or_else(|| {
            RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_COMPONENT_MISSING",
                "runtime component does not exist",
            ))
        })?;
        let before_revision = component.payload.revision;
        let schema = component.payload.schema.clone();
        let payload = component.payload.replaced(data.clone())?;
        let after_revision = payload.revision;
        component.payload = payload;
        crate::diagnostic_history::record(
            &mut self.mutations,
            RuntimeMutationRecord {
                step: self.step,
                component_id,
                schema,
                before_revision,
                after_revision,
                source: "runtime.api".to_string(),
            },
        );
        Ok(())
    }

    pub fn remove_actor(&mut self, actor_id: ActorId) -> Result<bool, RuntimeError> {
        self.ensure_active()?;
        let removed = self.actors.remove_actor(actor_id).is_some();
        debug!(?actor_id, removed, "runtime.actor.remove");
        Ok(removed)
    }

    pub fn detach_component(&mut self, component_id: ComponentId) -> Result<bool, RuntimeError> {
        self.ensure_active()?;
        let detached = self.actors.detach_component(component_id).is_some();
        debug!(?component_id, detached, "runtime.component.detach");
        Ok(detached)
    }

    pub fn add_state_machine(
        &mut self,
        definition: StateMachineDefinition,
    ) -> Result<(), RuntimeError> {
        self.ensure_active()?;
        debug!(
            machine_id = ?definition.id,
            owner = ?definition.owner,
            state_count = definition.states.len(),
            transition_count = definition.transitions.len(),
            "runtime.state_machine.add"
        );
        self.machines.add(definition)?;
        Ok(())
    }

    pub fn emit_event(
        &mut self,
        source: EventSource,
        payload: EventPayload,
    ) -> Result<(), RuntimeError> {
        self.ensure_active()?;
        let kind = payload.kind.clone();
        let event = RuntimeEvent {
            id: EventId(self.next_id()),
            source,
            step: self.step,
            sequence: 0,
            payload,
        };
        debug!(
            event_id = ?event.id,
            source = ?event.source,
            step = event.step,
            kind = %kind,
            "runtime.event.emit"
        );
        self.events.push(event);
        Ok(())
    }

    pub fn enqueue_event(&mut self, event: RuntimeEvent) -> Result<(), RuntimeError> {
        self.ensure_active()?;
        debug!(
            event_id = ?event.id,
            source = ?event.source,
            step = event.step,
            kind = %event.payload.kind,
            "runtime.event.enqueue"
        );
        self.events.push(event);
        Ok(())
    }

    pub fn schedule_event(
        &mut self,
        due_tick: u64,
        source: EventSource,
        payload: EventPayload,
    ) -> Result<DelayedEventId, RuntimeError> {
        self.ensure_active()?;
        let kind = payload.kind.clone();
        let source_for_log = source.clone();
        let event = ScheduledEvent {
            id: DelayedEventId(self.next_id()),
            due_tick,
            sequence: 0,
            source,
            payload,
        };
        let id = self.delayed_events.schedule(event);
        debug!(
            ?id,
            due_tick,
            source = ?source_for_log,
            kind = %kind,
            "runtime.delayed_event.schedule"
        );
        Ok(id)
    }

    pub fn cancel_delayed_event(&mut self, id: DelayedEventId) -> Result<bool, RuntimeError> {
        self.ensure_active()?;
        let cancelled = self.delayed_events.cancel(id);
        debug!(?id, cancelled, "runtime.delayed_event.cancel");
        Ok(cancelled)
    }

    pub fn emit_presentation(&mut self, command: PresentationCommand) -> Result<(), RuntimeError> {
        self.ensure_active()?;
        let sequence = self.presentation_sequence;
        self.presentation_sequence = sequence
            .checked_add(1)
            .ok_or_else(|| RuntimeError::message("ASTRA_RUNTIME_PRESENTATION_SEQUENCE_OVERFLOW"))?;
        debug!(
            step = self.step,
            sequence,
            command_kind = presentation_kind(&command),
            "runtime.presentation.emit"
        );
        crate::diagnostic_history::record(
            &mut self.presentation,
            PresentationRecord {
                step: self.step,
                sequence,
                command,
            },
        );
        Ok(())
    }

    pub fn insert_await_token(&mut self, token: AwaitToken) -> Result<(), RuntimeError> {
        self.ensure_active()?;
        debug!(
            token_id = ?token.token_id,
            requested_at_step = token.requested_at_step,
            timeout_step = ?token.timeout_step,
            "runtime.await.insert"
        );
        self.awaits.insert(token).map_err(RuntimeError::diagnostic)
    }

    pub(super) fn apply_input(&mut self, input: PlayerInput) -> Result<(), RuntimeError> {
        let mut payload = input.payload;
        if payload.kind.is_empty() {
            payload.kind = input.kind;
        }
        self.emit_event(EventSource::PlayerInput, payload)?;
        Ok(())
    }

    pub fn is_failed(&self) -> bool {
        self.failed
    }

    pub(super) fn ensure_active(&self) -> Result<(), RuntimeError> {
        if self.failed {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_SESSION_FAILED",
                "runtime execution failed; restore a saved state or recreate the world",
            )));
        }
        Ok(())
    }

    pub fn tick(&mut self, request: TickRequest) -> Result<TickReport, RuntimeError> {
        self.ensure_active()?;
        self.validate_tick_request(&request)?;
        let started = Instant::now();
        let performance_step = request.timing.fixed_step;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for ingress in request.ingress {
                match ingress.payload {
                    TickIngress::PlayerInput(input) => self.apply_input(input)?,
                    TickIngress::AwaitCompletion(result) => self.submit_await_result(result),
                }
            }
            let report = self.tick_validated(request.timing)?;
            self.required_tick_mode = TickMode::Live;
            Ok(report)
        }))
        .unwrap_or_else(|_| {
            Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_EXECUTION_PANIC",
                "runtime action execution panicked",
            )))
        });
        self.failed = result.is_err();
        if self.failed {
            self.tasks.scope().cancel();
        }
        trace!(
            event = "runtime.tick.execution.performance",
            step = performance_step,
            execution_ns = started.elapsed().as_nanos() as u64,
            succeeded = result.is_ok(),
            "measured RuntimeWorld tick execution"
        );
        result
    }
}
