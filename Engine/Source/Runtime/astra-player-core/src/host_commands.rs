use super::*;

pub const WEB_PLAYER_LIVE_EVIDENCE_SCHEMA: &str = "astra.player_web_live_evidence.v2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PlayerAction {
    Advance,
    ChooseIndex { index: usize },
    OpenSystemPage { page: String },
    Back,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerActionBinding {
    pub input: String,
    pub action: PlayerAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerActionMap {
    pub schema: String,
    pub bindings: Vec<PlayerActionBinding>,
}

impl PlayerActionMap {
    pub fn standard() -> Self {
        let mut bindings = vec![
            PlayerActionBinding {
                input: "Enter".into(),
                action: PlayerAction::Advance,
            },
            PlayerActionBinding {
                input: "Space".into(),
                action: PlayerAction::Advance,
            },
            PlayerActionBinding {
                input: "Escape".into(),
                action: PlayerAction::Back,
            },
            PlayerActionBinding {
                input: "KeyB".into(),
                action: PlayerAction::OpenSystemPage {
                    page: "backlog".into(),
                },
            },
        ];
        bindings.extend((0..9).map(|index| PlayerActionBinding {
            input: format!("Digit{}", index + 1),
            action: PlayerAction::ChooseIndex { index },
        }));
        Self {
            schema: "astra.player_action_map.v1".into(),
            bindings,
        }
    }

    pub fn keyboard(&self, physical_key: &str) -> Option<PlayerAction> {
        self.bindings
            .iter()
            .find(|binding| binding.input == physical_key)
            .map(|binding| binding.action.clone())
    }
}

/// Logical resource identity used only inside the Player command stream. It is
/// deliberately unrelated to platform handles and may safely cross runtime
/// provider boundaries.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct PlayerHostResourceId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayerPackageSource {
    Bundled {
        relative_path: String,
        expected_hash: String,
    },
    UserAuthorized {
        expected_hash: String,
    },
    HttpsRange {
        url: String,
        expected_hash: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayerDecodeKind {
    Audio,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayerDecodeStreamAction {
    OneShot,
    Start,
    Next,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerHostCommand {
    OpenPackage {
        sequence: u64,
        source: PlayerPackageSource,
        package: PlayerHostResourceId,
    },
    ReadPackageRange {
        sequence: u64,
        package: PlayerHostResourceId,
        offset: u64,
        length: u32,
    },
    ClosePackage {
        sequence: u64,
        package: PlayerHostResourceId,
    },
    BeginSave {
        sequence: u64,
        slot: String,
        transaction: PlayerHostResourceId,
    },
    WriteSave {
        sequence: u64,
        transaction: PlayerHostResourceId,
        bytes: Vec<u8>,
    },
    CommitSave {
        sequence: u64,
        transaction: PlayerHostResourceId,
    },
    AbortSave {
        sequence: u64,
        transaction: PlayerHostResourceId,
    },
    ReadSave {
        sequence: u64,
        slot: String,
    },
    ListSaves {
        sequence: u64,
    },
    DeleteSave {
        sequence: u64,
        slot: String,
    },
    OpenDecode {
        sequence: u64,
        session: PlayerHostResourceId,
        kind: PlayerDecodeKind,
    },
    Decode {
        sequence: u64,
        request_sequence: u64,
        session: PlayerHostResourceId,
        kind: PlayerDecodeKind,
        codec: String,
        description: Vec<u8>,
        sample_rate: Option<u32>,
        channels: Option<u16>,
        coded_width: Option<u32>,
        coded_height: Option<u32>,
        keyframe: bool,
        stream_action: PlayerDecodeStreamAction,
        bytes: astra_byte_source::OwnedByteBuffer,
    },
    CloseDecode {
        sequence: u64,
        session: PlayerHostResourceId,
    },
    PresentRgba {
        sequence: u64,
        surface: PlayerHostResourceId,
        width: u32,
        height: u32,
        rgba8: Vec<u8>,
    },
    PresentScene {
        sequence: u64,
        surface: PlayerHostResourceId,
        width: u32,
        height: u32,
        clear_rgba: [u8; 4],
        commands: Vec<astra_media_core::SceneCommand>,
        semantics: Option<astra_ui_core::UiSemanticSnapshot>,
    },
    CaptureSurface {
        sequence: u64,
        surface: PlayerHostResourceId,
    },
}

impl PlayerHostCommand {
    pub fn sequence(&self) -> u64 {
        match self {
            Self::OpenPackage { sequence, .. }
            | Self::ReadPackageRange { sequence, .. }
            | Self::ClosePackage { sequence, .. }
            | Self::BeginSave { sequence, .. }
            | Self::WriteSave { sequence, .. }
            | Self::CommitSave { sequence, .. }
            | Self::AbortSave { sequence, .. }
            | Self::ReadSave { sequence, .. }
            | Self::ListSaves { sequence }
            | Self::DeleteSave { sequence, .. }
            | Self::OpenDecode { sequence, .. }
            | Self::Decode { sequence, .. }
            | Self::CloseDecode { sequence, .. }
            | Self::PresentRgba { sequence, .. }
            | Self::PresentScene { sequence, .. }
            | Self::CaptureSurface { sequence, .. } => *sequence,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerHostCommandBatch {
    pub commands: Vec<PlayerHostCommand>,
}

impl PlayerHostCommandBatch {
    pub fn new(commands: Vec<PlayerHostCommand>) -> Result<Self, PlayerHostCommandError> {
        let mut previous = 0_u64;
        for command in &commands {
            let sequence = command.sequence();
            if sequence == 0 || sequence <= previous {
                return Err(PlayerHostCommandError::SequenceNotStrictlyIncreasing);
            }
            previous = sequence;
        }
        Ok(Self { commands })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerHostCommandResult {
    PackageOpened {
        package: PlayerHostResourceId,
    },
    PackageRange {
        package: PlayerHostResourceId,
        bytes: Vec<u8>,
    },
    PackageClosed {
        package: PlayerHostResourceId,
    },
    SaveStarted {
        transaction: PlayerHostResourceId,
    },
    SaveCommitted {
        transaction: PlayerHostResourceId,
        hash: String,
    },
    SaveRead {
        bytes: Vec<u8>,
    },
    SaveList {
        slots: Vec<String>,
    },
    DecodeOpened {
        session: PlayerHostResourceId,
    },
    Decoded {
        session: PlayerHostResourceId,
        output: astra_platform::DecodeOutput,
    },
    DecodeClosed {
        session: PlayerHostResourceId,
    },
    Presented {
        surface: PlayerHostResourceId,
    },
    Captured {
        surface: PlayerHostResourceId,
        width: u32,
        height: u32,
        rgba8: Vec<u8>,
    },
    Unit,
}

pub trait PlayerHostCommandSink {
    type Error;

    fn execute<'a>(
        &'a mut self,
        command: &'a PlayerHostCommand,
    ) -> Pin<Box<dyn Future<Output = Result<PlayerHostCommandResult, Self::Error>> + 'a>>;
}

pub struct PlayerHostCommandExecutor<S> {
    pub(super) sink: S,
    pub(super) last_sequence: u64,
}

pub struct PlayerSaveTransactionPlan {
    pub begin: PlayerHostCommandBatch,
    pub write: PlayerHostCommandBatch,
    pub commit: PlayerHostCommandBatch,
    pub abort: PlayerHostCommandBatch,
}

#[derive(Debug)]
pub enum PlayerSaveTransactionError<E> {
    Begin(PlayerHostCommandExecutionError<E>),
    Write {
        source: PlayerHostCommandExecutionError<E>,
        abort: Option<PlayerHostCommandExecutionError<E>>,
    },
    Commit {
        source: PlayerHostCommandExecutionError<E>,
        abort: Option<PlayerHostCommandExecutionError<E>>,
    },
}

impl<E: std::fmt::Display> std::fmt::Display for PlayerSaveTransactionError<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Begin(error) => write!(formatter, "save begin failed: {error}"),
            Self::Write { source, abort } => {
                write_transaction_failure(formatter, "save write", source, abort.as_ref())
            }
            Self::Commit { source, abort } => {
                write_transaction_failure(formatter, "save commit", source, abort.as_ref())
            }
        }
    }
}

pub(super) fn write_transaction_failure<E: std::fmt::Display>(
    formatter: &mut std::fmt::Formatter<'_>,
    operation: &str,
    source: &PlayerHostCommandExecutionError<E>,
    abort: Option<&PlayerHostCommandExecutionError<E>>,
) -> std::fmt::Result {
    write!(formatter, "{operation} failed: {source}")?;
    if let Some(abort) = abort {
        write!(formatter, "; save abort also failed: {abort}")?;
    }
    Ok(())
}

impl<E: std::error::Error + 'static> std::error::Error for PlayerSaveTransactionError<E> {}

impl<S> PlayerHostCommandExecutor<S> {
    pub fn new(sink: S) -> Self {
        Self {
            sink,
            last_sequence: 0,
        }
    }
    pub fn sink(&self) -> &S {
        &self.sink
    }
    pub fn sink_mut(&mut self) -> &mut S {
        &mut self.sink
    }
}

impl<S: PlayerHostCommandSink> PlayerHostCommandExecutor<S> {
    pub async fn execute_batch(
        &mut self,
        batch: PlayerHostCommandBatch,
    ) -> Result<Vec<PlayerHostCommandResult>, PlayerHostCommandExecutionError<S::Error>> {
        let mut results = Vec::with_capacity(batch.commands.len());
        for command in &batch.commands {
            if command.sequence() <= self.last_sequence {
                return Err(PlayerHostCommandExecutionError::SequenceNotStrictlyIncreasing);
            }
            let result = self
                .sink
                .execute(command)
                .await
                .map_err(PlayerHostCommandExecutionError::Sink)?;
            self.last_sequence = command.sequence();
            results.push(result);
        }
        Ok(results)
    }

    pub async fn execute_save_transaction(
        &mut self,
        plan: PlayerSaveTransactionPlan,
    ) -> Result<(), PlayerSaveTransactionError<S::Error>> {
        self.execute_batch(plan.begin)
            .await
            .map_err(PlayerSaveTransactionError::Begin)?;
        if let Err(source) = self.execute_batch(plan.write).await {
            let abort = self.execute_batch(plan.abort).await.err();
            return Err(PlayerSaveTransactionError::Write { source, abort });
        }
        if let Err(source) = self.execute_batch(plan.commit).await {
            let abort = self.execute_batch(plan.abort).await.err();
            return Err(PlayerSaveTransactionError::Commit { source, abort });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerHostCommandExecutionError<E> {
    SequenceNotStrictlyIncreasing,
    Sink(E),
}

impl<E: std::fmt::Display> std::fmt::Display for PlayerHostCommandExecutionError<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SequenceNotStrictlyIncreasing => {
                formatter.write_str("Player host command sequence is not strictly increasing")
            }
            Self::Sink(error) => write!(formatter, "Player host command failed: {error}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for PlayerHostCommandExecutionError<E> {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerHostCommandError {
    SequenceNotStrictlyIncreasing,
}

impl std::fmt::Display for PlayerHostCommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SequenceNotStrictlyIncreasing => {
                formatter.write_str("player host command sequence is not strictly increasing")
            }
        }
    }
}

impl std::error::Error for PlayerHostCommandError {}
