use super::*;

/// A display projection, never an authoritative save/restore state.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeVnStateView {
    pub backlog_count: usize,
    state: VnRuntimeState,
}

impl NativeVnStateView {
    pub(crate) fn project(state: &VnRuntimeState) -> Self {
        let page = state.system_stack.last().map(|frame| frame.page);
        let routes = page == Some(SystemPageKind::RouteChart) || state.cursor.is_none();
        Self {
            backlog_count: state.backlog.len(),
            state: VnRuntimeState {
                schema: state.schema.clone(),
                revision: state.revision,
                instance_id: state.instance_id.clone(),
                profile: state.profile.clone(),
                locale: state.locale.clone(),
                cursor: state.cursor.clone(),
                call_stack: Vec::new(),
                system_stack: state.system_stack.clone(),
                system: state.system.clone(),
                pending_choice: state.pending_choice.clone(),
                variables: BTreeMap::new(),
                backlog: state
                    .backlog
                    .iter()
                    .skip(if page == Some(SystemPageKind::Backlog) {
                        0
                    } else {
                        state.backlog.len().saturating_sub(1)
                    })
                    .cloned()
                    .collect(),
                read_state: Default::default(),
                voice_replay: if page == Some(SystemPageKind::VoiceReplay) {
                    state.voice_replay.clone()
                } else {
                    Default::default()
                },
                route_coverage: if routes {
                    state.route_coverage.clone()
                } else {
                    Default::default()
                },
                route_flags: if routes {
                    state.route_flags.clone()
                } else {
                    Default::default()
                },
                wait_sequence: 0,
                pending_wait: state.pending_wait.clone(),
            },
        }
    }

    /// Move the projected fields into the existing read-only VN UI model.
    pub fn into_display_state(self) -> VnRuntimeState {
        self.state
    }

    pub(crate) fn state(&self) -> &VnRuntimeState {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history() -> VnRuntimeState {
        let compiled = compile_astra_project(
            [AstraSource::story("view.astra", "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:hello speaker:narrator #@id hello\n")],
            Default::default(),
        ).unwrap();
        let mut runtime = CoreVnRuntime::new(compiled, VnRunConfig::classic("en")).unwrap();
        runtime
            .apply(runtime.default_launch_command().unwrap())
            .unwrap();
        let mut state = runtime.state().clone();
        let entry = state.backlog.last().unwrap().clone();
        state.backlog = (0..4096)
            .map(|index| {
                let mut entry = entry.clone();
                entry.command_id = format!("line.{index}");
                entry
            })
            .collect();
        state.voice_replay.insert(
            "voice.one".into(),
            astra_vn_core::VoiceReplayEntry {
                voice: "voice.one".into(),
                line_key: "hello".into(),
                speaker: Some("narrator".into()),
            },
        );
        state
            .variables
            .insert("private".into(), BTreeMap::from([("secret".into(), 42)]));
        state.read_state.insert("hello".into());
        state
    }

    fn page(state: &mut VnRuntimeState, page: SystemPageKind) {
        state.system_stack = vec![astra_vn_core::VnSystemFrame {
            return_to: state.cursor.clone().unwrap(),
            return_wait: state.pending_wait.clone(),
            return_choice: state.pending_choice.clone(),
            page,
        }];
    }

    #[test]
    fn dialogue_projection_does_not_expand_history_or_private_gameplay_data() {
        let state = history();
        let view = NativeVnStateView::project(&state);
        assert_eq!(view.backlog_count, 4096);
        let display = view.state();
        assert_eq!(display.backlog.len(), 1);
        assert_eq!(display.backlog.last(), state.backlog.last());
        assert!(display.voice_replay.is_empty());
        assert!(display.route_coverage.is_empty());
        assert!(display.route_flags.is_empty());
        assert!(display.variables.is_empty());
        assert!(display.read_state.is_empty());
        assert!(display.call_stack.is_empty());
        assert_eq!(display.wait_sequence, 0);
        assert_eq!(display.cursor, state.cursor);
        assert_eq!(display.pending_wait, state.pending_wait);
        assert_eq!(display.system, state.system);
        let abi = runtime_live_vn_state(&view);
        assert_eq!(abi.backlog_count, 4096);
        assert_eq!(abi.backlog.len(), 1);
        assert_eq!(abi.backlog[0].command_id, "line.4095");
    }

    #[test]
    fn system_pages_and_terminal_project_only_their_requested_history() {
        let mut state = history();
        assert!(!state.route_coverage.is_empty());
        assert!(!state.route_flags.is_empty());
        page(&mut state, SystemPageKind::Backlog);
        let backlog = NativeVnStateView::project(&state);
        assert_eq!(backlog.state().backlog, state.backlog);
        assert!(backlog.state().voice_replay.is_empty());
        page(&mut state, SystemPageKind::VoiceReplay);
        let voice = NativeVnStateView::project(&state);
        assert_eq!(voice.state().backlog.len(), 1);
        assert_eq!(voice.state().voice_replay, state.voice_replay);
        page(&mut state, SystemPageKind::RouteChart);
        let routes = NativeVnStateView::project(&state);
        assert_eq!(routes.state().route_coverage, state.route_coverage);
        assert_eq!(routes.state().route_flags, state.route_flags);
        assert!(routes.state().voice_replay.is_empty());
        state.system_stack.clear();
        assert!(NativeVnStateView::project(&state)
            .state()
            .route_flags
            .is_empty());
        state.cursor = None;
        let terminal = NativeVnStateView::project(&state).into_display_state();
        assert_eq!(terminal.route_coverage, state.route_coverage);
        assert_eq!(terminal.route_flags, state.route_flags);
        assert_eq!(terminal.backlog.len(), 1);
        assert_eq!(state.backlog.len(), 4096);
    }
}
