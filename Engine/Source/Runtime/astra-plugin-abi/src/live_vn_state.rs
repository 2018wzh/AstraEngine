#[cfg(feature = "ffi")]
use abi_stable::{
    std_types::{ROption, RString, RVec},
    StableAbi,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnState {
    pub backlog_count: usize,
    pub revision: u64,
    pub instance_id: String,
    pub profile: String,
    pub locale: String,
    pub cursor: Option<RuntimeLiveVnCursor>,
    pub system_stack: Vec<RuntimeLiveVnSystemFrame>,
    pub system: RuntimeLiveVnSystemState,
    pub pending_choice: Option<RuntimeLiveVnPendingChoice>,
    pub backlog: Vec<RuntimeLiveVnBacklogEntry>,
    pub voice_replay: Vec<RuntimeLiveVnVoiceReplayEntry>,
    pub route_coverage: Vec<String>,
    pub route_flags: Vec<RuntimeLiveVnRouteFlag>,
    pub pending_wait: Option<RuntimeLiveVnWait>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnStep {
    pub coverage_reached: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnCursor {
    pub story_id: String,
    pub state_id: String,
    pub scene_id: String,
    pub command_id: String,
    pub ordinal: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnSystemFrame {
    pub return_to: RuntimeLiveVnCursor,
    pub return_wait: Option<RuntimeLiveVnWait>,
    pub return_choice: Option<RuntimeLiveVnPendingChoice>,
    pub page: super::RuntimeLiveSystemPage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnSystemState {
    pub auto_enabled: bool,
    pub skip_mode: RuntimeLiveVnSkipMode,
    pub config: Vec<RuntimeLiveVnStringEntry>,
    pub gallery_unlocks: Vec<String>,
    pub replay_unlocks: Vec<String>,
    pub reading_mode: RuntimeLiveVnReadingMode,
    pub audio_enabled: bool,
    pub skip_allowed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveVnSkipMode {
    None,
    Read,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveVnReadingMode {
    Hidden,
    Manual,
    FastForward,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnStringEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnPendingChoice {
    pub choice_id: String,
    pub key: String,
    pub options: Vec<super::RuntimeLiveChoiceOption>,
    pub enabled_option_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnBacklogEntry {
    pub command_id: String,
    pub key: String,
    pub speaker: Option<String>,
    pub voice: Option<String>,
    pub story_id: String,
    pub state_id: String,
    pub route_position: usize,
    pub read: bool,
    pub window: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnVoiceReplayEntry {
    pub id: String,
    pub voice: String,
    pub line_key: String,
    pub speaker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnRouteFlag {
    pub id: String,
    pub kind: RuntimeLiveVnRouteFlagKind,
    pub source: String,
    pub target: String,
    pub count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveVnRouteFlagKind {
    Launch,
    Choice,
    Jump,
    Branch,
    Call,
    Return,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnWait {
    pub kind: RuntimeLiveVnWaitKind,
    pub fence: String,
    pub command_id: String,
    pub await_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveVnWaitKind {
    Dialogue,
    Choice,
    SystemPage,
    Fence,
    Timer,
    TimelineComplete,
    MovieEnd,
    VoiceEnd,
    Input,
}

#[cfg(feature = "ffi")]
macro_rules! ffi_struct {
    ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        #[repr(C)]
        #[derive(Debug, Clone, StableAbi)]
        pub struct $name { $(pub $field: $ty),* }
    };
}

#[cfg(feature = "ffi")]
macro_rules! ffi_enum {
    ($name:ident { $($variant:ident),* $(,)? }) => {
        #[repr(u8)]
        #[derive(Debug, Clone, Copy, StableAbi)]
        pub enum $name { $($variant),* }
    };
}

#[cfg(feature = "ffi")]
ffi_struct!(FfiRuntimeLiveVnState { backlog_count: usize, revision: u64, instance_id: RString, profile: RString, locale: RString, cursor: ROption<FfiRuntimeLiveVnCursor>, system_stack: RVec<FfiRuntimeLiveVnSystemFrame>, system: FfiRuntimeLiveVnSystemState, pending_choice: ROption<FfiRuntimeLiveVnPendingChoice>, backlog: RVec<FfiRuntimeLiveVnBacklogEntry>, voice_replay: RVec<FfiRuntimeLiveVnVoiceReplayEntry>, route_coverage: RVec<RString>, route_flags: RVec<FfiRuntimeLiveVnRouteFlag>, pending_wait: ROption<FfiRuntimeLiveVnWait> });
#[cfg(feature = "ffi")]
ffi_struct!(FfiRuntimeLiveVnStep { coverage_reached: RVec<RString> });
#[cfg(feature = "ffi")]
ffi_struct!(FfiRuntimeLiveVnCursor {
    story_id: RString,
    state_id: RString,
    scene_id: RString,
    command_id: RString,
    ordinal: usize
});
#[cfg(feature = "ffi")]
ffi_struct!(FfiRuntimeLiveVnSystemFrame { return_to: FfiRuntimeLiveVnCursor, return_wait: ROption<FfiRuntimeLiveVnWait>, return_choice: ROption<FfiRuntimeLiveVnPendingChoice>, page: super::FfiRuntimeLiveSystemPage });
#[cfg(feature = "ffi")]
ffi_struct!(FfiRuntimeLiveVnSystemState { auto_enabled: bool, skip_mode: FfiRuntimeLiveVnSkipMode, config: RVec<FfiRuntimeLiveVnStringEntry>, gallery_unlocks: RVec<RString>, replay_unlocks: RVec<RString>, reading_mode: FfiRuntimeLiveVnReadingMode, audio_enabled: bool, skip_allowed: bool });
#[cfg(feature = "ffi")]
ffi_enum!(FfiRuntimeLiveVnSkipMode { None, Read, All });
#[cfg(feature = "ffi")]
ffi_enum!(FfiRuntimeLiveVnReadingMode {
    Hidden,
    Manual,
    FastForward
});
#[cfg(feature = "ffi")]
ffi_struct!(FfiRuntimeLiveVnStringEntry {
    key: RString,
    value: RString
});
#[cfg(feature = "ffi")]
ffi_struct!(FfiRuntimeLiveVnPendingChoice { choice_id: RString, key: RString, options: RVec<super::FfiRuntimeLiveChoiceOption>, enabled_option_ids: RVec<RString> });
#[cfg(feature = "ffi")]
ffi_struct!(FfiRuntimeLiveVnBacklogEntry { command_id: RString, key: RString, speaker: ROption<RString>, voice: ROption<RString>, story_id: RString, state_id: RString, route_position: usize, read: bool, window: ROption<RString> });
#[cfg(feature = "ffi")]
ffi_struct!(FfiRuntimeLiveVnVoiceReplayEntry { id: RString, voice: RString, line_key: RString, speaker: ROption<RString> });
#[cfg(feature = "ffi")]
ffi_struct!(FfiRuntimeLiveVnRouteFlag {
    id: RString,
    kind: FfiRuntimeLiveVnRouteFlagKind,
    source: RString,
    target: RString,
    count: u32
});
#[cfg(feature = "ffi")]
ffi_enum!(FfiRuntimeLiveVnRouteFlagKind {
    Launch,
    Choice,
    Jump,
    Branch,
    Call,
    Return
});
#[cfg(feature = "ffi")]
ffi_struct!(FfiRuntimeLiveVnWait { kind: FfiRuntimeLiveVnWaitKind, fence: RString, command_id: RString, await_id: ROption<RString> });
#[cfg(feature = "ffi")]
ffi_enum!(FfiRuntimeLiveVnWaitKind {
    Dialogue,
    Choice,
    SystemPage,
    Fence,
    Timer,
    TimelineComplete,
    MovieEnd,
    VoiceEnd,
    Input
});

#[cfg(feature = "ffi")]
macro_rules! enum_bridge {
    ($rust:ident, $ffi:ident { $($variant:ident),+ $(,)? }) => {
        impl From<$rust> for $ffi { fn from(value: $rust) -> Self { match value { $($rust::$variant => Self::$variant),+ } } }
        impl From<$ffi> for $rust { fn from(value: $ffi) -> Self { match value { $($ffi::$variant => Self::$variant),+ } } }
    };
}

#[cfg(feature = "ffi")]
enum_bridge!(
    RuntimeLiveVnSkipMode,
    FfiRuntimeLiveVnSkipMode { None, Read, All }
);
#[cfg(feature = "ffi")]
enum_bridge!(
    RuntimeLiveVnReadingMode,
    FfiRuntimeLiveVnReadingMode {
        Hidden,
        Manual,
        FastForward
    }
);
#[cfg(feature = "ffi")]
enum_bridge!(
    RuntimeLiveVnRouteFlagKind,
    FfiRuntimeLiveVnRouteFlagKind {
        Launch,
        Choice,
        Jump,
        Branch,
        Call,
        Return
    }
);
#[cfg(feature = "ffi")]
enum_bridge!(
    RuntimeLiveVnWaitKind,
    FfiRuntimeLiveVnWaitKind {
        Dialogue,
        Choice,
        SystemPage,
        Fence,
        Timer,
        TimelineComplete,
        MovieEnd,
        VoiceEnd,
        Input
    }
);

#[cfg(feature = "ffi")]
impl RuntimeLiveVnState {
    pub fn into_ffi(self) -> FfiRuntimeLiveVnState {
        FfiRuntimeLiveVnState {
            backlog_count: self.backlog_count,
            revision: self.revision,
            instance_id: self.instance_id.into(),
            profile: self.profile.into(),
            locale: self.locale.into(),
            cursor: self.cursor.map(Into::into).into(),
            system_stack: self
                .system_stack
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            system: self.system.into(),
            pending_choice: self.pending_choice.map(Into::into).into(),
            backlog: self
                .backlog
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            voice_replay: self
                .voice_replay
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            route_coverage: self
                .route_coverage
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            route_flags: self
                .route_flags
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            pending_wait: self.pending_wait.map(Into::into).into(),
        }
    }
}

#[cfg(feature = "ffi")]
impl FfiRuntimeLiveVnState {
    pub fn into_runtime(self) -> RuntimeLiveVnState {
        RuntimeLiveVnState {
            backlog_count: self.backlog_count,
            revision: self.revision,
            instance_id: self.instance_id.into(),
            profile: self.profile.into(),
            locale: self.locale.into(),
            cursor: self.cursor.into_option().map(Into::into),
            system_stack: self.system_stack.into_iter().map(Into::into).collect(),
            system: self.system.into(),
            pending_choice: self.pending_choice.into_option().map(Into::into),
            backlog: self.backlog.into_iter().map(Into::into).collect(),
            voice_replay: self.voice_replay.into_iter().map(Into::into).collect(),
            route_coverage: self.route_coverage.into_iter().map(Into::into).collect(),
            route_flags: self.route_flags.into_iter().map(Into::into).collect(),
            pending_wait: self.pending_wait.into_option().map(Into::into),
        }
    }
}

#[cfg(feature = "ffi")]
impl RuntimeLiveVnStep {
    pub fn into_ffi(self) -> FfiRuntimeLiveVnStep {
        FfiRuntimeLiveVnStep {
            coverage_reached: self
                .coverage_reached
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
        }
    }
}

#[cfg(feature = "ffi")]
impl FfiRuntimeLiveVnStep {
    pub fn into_runtime(self) -> RuntimeLiveVnStep {
        RuntimeLiveVnStep {
            coverage_reached: self.coverage_reached.into_iter().map(Into::into).collect(),
        }
    }
}

#[cfg(feature = "ffi")]
macro_rules! string_struct_bridge {
    ($rust:ident, $ffi:ident { $($field:ident),+ $(,)? }) => {
        impl From<$rust> for $ffi { fn from(value: $rust) -> Self { Self { $($field: value.$field.into()),+ } } }
        impl From<$ffi> for $rust { fn from(value: $ffi) -> Self { Self { $($field: value.$field.into()),+ } } }
    };
}

#[cfg(feature = "ffi")]
string_struct_bridge!(
    RuntimeLiveVnCursor,
    FfiRuntimeLiveVnCursor {
        story_id,
        state_id,
        scene_id,
        command_id,
        ordinal
    }
);
#[cfg(feature = "ffi")]
string_struct_bridge!(
    RuntimeLiveVnStringEntry,
    FfiRuntimeLiveVnStringEntry { key, value }
);

#[cfg(feature = "ffi")]
impl From<RuntimeLiveVnSystemFrame> for FfiRuntimeLiveVnSystemFrame {
    fn from(v: RuntimeLiveVnSystemFrame) -> Self {
        Self {
            return_to: v.return_to.into(),
            return_wait: v.return_wait.map(Into::into).into(),
            return_choice: v.return_choice.map(Into::into).into(),
            page: v.page.into(),
        }
    }
}
#[cfg(feature = "ffi")]
impl From<FfiRuntimeLiveVnSystemFrame> for RuntimeLiveVnSystemFrame {
    fn from(v: FfiRuntimeLiveVnSystemFrame) -> Self {
        Self {
            return_to: v.return_to.into(),
            return_wait: v.return_wait.into_option().map(Into::into),
            return_choice: v.return_choice.into_option().map(Into::into),
            page: v.page.into(),
        }
    }
}

#[cfg(feature = "ffi")]
impl From<RuntimeLiveVnSystemState> for FfiRuntimeLiveVnSystemState {
    fn from(v: RuntimeLiveVnSystemState) -> Self {
        Self {
            auto_enabled: v.auto_enabled,
            skip_mode: v.skip_mode.into(),
            config: v
                .config
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            gallery_unlocks: v
                .gallery_unlocks
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            replay_unlocks: v
                .replay_unlocks
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            reading_mode: v.reading_mode.into(),
            audio_enabled: v.audio_enabled,
            skip_allowed: v.skip_allowed,
        }
    }
}
#[cfg(feature = "ffi")]
impl From<FfiRuntimeLiveVnSystemState> for RuntimeLiveVnSystemState {
    fn from(v: FfiRuntimeLiveVnSystemState) -> Self {
        Self {
            auto_enabled: v.auto_enabled,
            skip_mode: v.skip_mode.into(),
            config: v.config.into_iter().map(Into::into).collect(),
            gallery_unlocks: v.gallery_unlocks.into_iter().map(Into::into).collect(),
            replay_unlocks: v.replay_unlocks.into_iter().map(Into::into).collect(),
            reading_mode: v.reading_mode.into(),
            audio_enabled: v.audio_enabled,
            skip_allowed: v.skip_allowed,
        }
    }
}

#[cfg(feature = "ffi")]
impl From<RuntimeLiveVnPendingChoice> for FfiRuntimeLiveVnPendingChoice {
    fn from(v: RuntimeLiveVnPendingChoice) -> Self {
        Self {
            choice_id: v.choice_id.into(),
            key: v.key.into(),
            options: v
                .options
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            enabled_option_ids: v
                .enabled_option_ids
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
        }
    }
}
#[cfg(feature = "ffi")]
impl From<FfiRuntimeLiveVnPendingChoice> for RuntimeLiveVnPendingChoice {
    fn from(v: FfiRuntimeLiveVnPendingChoice) -> Self {
        Self {
            choice_id: v.choice_id.into(),
            key: v.key.into(),
            options: v.options.into_iter().map(Into::into).collect(),
            enabled_option_ids: v.enabled_option_ids.into_iter().map(Into::into).collect(),
        }
    }
}

#[cfg(feature = "ffi")]
impl From<RuntimeLiveVnBacklogEntry> for FfiRuntimeLiveVnBacklogEntry {
    fn from(v: RuntimeLiveVnBacklogEntry) -> Self {
        Self {
            command_id: v.command_id.into(),
            key: v.key.into(),
            speaker: v.speaker.map(Into::into).into(),
            voice: v.voice.map(Into::into).into(),
            story_id: v.story_id.into(),
            state_id: v.state_id.into(),
            route_position: v.route_position,
            read: v.read,
            window: v.window.map(Into::into).into(),
        }
    }
}
#[cfg(feature = "ffi")]
impl From<FfiRuntimeLiveVnBacklogEntry> for RuntimeLiveVnBacklogEntry {
    fn from(v: FfiRuntimeLiveVnBacklogEntry) -> Self {
        Self {
            command_id: v.command_id.into(),
            key: v.key.into(),
            speaker: v.speaker.into_option().map(Into::into),
            voice: v.voice.into_option().map(Into::into),
            story_id: v.story_id.into(),
            state_id: v.state_id.into(),
            route_position: v.route_position,
            read: v.read,
            window: v.window.into_option().map(Into::into),
        }
    }
}

#[cfg(feature = "ffi")]
impl From<RuntimeLiveVnVoiceReplayEntry> for FfiRuntimeLiveVnVoiceReplayEntry {
    fn from(v: RuntimeLiveVnVoiceReplayEntry) -> Self {
        Self {
            id: v.id.into(),
            voice: v.voice.into(),
            line_key: v.line_key.into(),
            speaker: v.speaker.map(Into::into).into(),
        }
    }
}
#[cfg(feature = "ffi")]
impl From<FfiRuntimeLiveVnVoiceReplayEntry> for RuntimeLiveVnVoiceReplayEntry {
    fn from(v: FfiRuntimeLiveVnVoiceReplayEntry) -> Self {
        Self {
            id: v.id.into(),
            voice: v.voice.into(),
            line_key: v.line_key.into(),
            speaker: v.speaker.into_option().map(Into::into),
        }
    }
}

#[cfg(feature = "ffi")]
impl From<RuntimeLiveVnRouteFlag> for FfiRuntimeLiveVnRouteFlag {
    fn from(v: RuntimeLiveVnRouteFlag) -> Self {
        Self {
            id: v.id.into(),
            kind: v.kind.into(),
            source: v.source.into(),
            target: v.target.into(),
            count: v.count,
        }
    }
}
#[cfg(feature = "ffi")]
impl From<FfiRuntimeLiveVnRouteFlag> for RuntimeLiveVnRouteFlag {
    fn from(v: FfiRuntimeLiveVnRouteFlag) -> Self {
        Self {
            id: v.id.into(),
            kind: v.kind.into(),
            source: v.source.into(),
            target: v.target.into(),
            count: v.count,
        }
    }
}

#[cfg(feature = "ffi")]
impl From<RuntimeLiveVnWait> for FfiRuntimeLiveVnWait {
    fn from(v: RuntimeLiveVnWait) -> Self {
        Self {
            kind: v.kind.into(),
            fence: v.fence.into(),
            command_id: v.command_id.into(),
            await_id: v.await_id.map(Into::into).into(),
        }
    }
}
#[cfg(feature = "ffi")]
impl From<FfiRuntimeLiveVnWait> for RuntimeLiveVnWait {
    fn from(v: FfiRuntimeLiveVnWait) -> Self {
        Self {
            kind: v.kind.into(),
            fence: v.fence.into(),
            command_id: v.command_id.into(),
            await_id: v.await_id.into_option().map(Into::into),
        }
    }
}
