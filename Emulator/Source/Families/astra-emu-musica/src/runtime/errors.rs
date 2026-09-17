use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MusicaRuntimeError {
    #[error("ASTRA_EMU_MUSICA_RUNTIME_STATE: runtime state is invalid")]
    State,
    #[error("ASTRA_EMU_MUSICA_RUNTIME_PC: program counter is outside the script")]
    ProgramCounter,
    #[error("ASTRA_EMU_MUSICA_RUNTIME_LABEL: branch label is missing or duplicated")]
    Label,
    #[error("ASTRA_EMU_MUSICA_RUNTIME_OPERAND: command operands do not match the verified schema")]
    Operand,
    #[error(
        "ASTRA_EMU_MUSICA_RUNTIME_OPCODE: command `{opcode}` at ordinal {ordinal} is not verified"
    )]
    UnsupportedOpcode { opcode: String, ordinal: u32 },
    #[error("ASTRA_EMU_MUSICA_RUNTIME_NON_YIELDING_CYCLE: command execution repeated a control state without yielding")]
    NonYieldingCycle,
    #[error("ASTRA_EMU_MUSICA_RUNTIME_WAIT: runtime is awaiting an unresolved token")]
    Waiting,
    #[error("ASTRA_EMU_MUSICA_RUNTIME_OVERFLOW: deterministic counter overflowed")]
    Overflow,
    #[error("ASTRA_EMU_MUSICA_NATIVE_SAVE_FORMAT: native save data is malformed")]
    NativeSaveFormat,
    #[error("ASTRA_EMU_MUSICA_RUNTIME_CHAIN: chain target is outside the script mount")]
    ChainTarget,
    #[error("ASTRA_EMU_MUSICA_RUNTIME_AUDIO_RESOURCE: audio resource specification is invalid")]
    AudioResource,
    #[error("ASTRA_EMU_MUSICA_RUNTIME_EFFECT: effect operands or timeline are invalid")]
    Effect,
    #[error("ASTRA_EMU_MUSICA_RUNTIME_PANEL: panel operands or mode are invalid")]
    Panel,
    #[error("ASTRA_EMU_MUSICA_RUNTIME_PRAGMA: unsupported or malformed pragma")]
    Pragma,
    #[error("ASTRA_EMU_MUSICA_RUNTIME_SCREEN_SHAKE: invalid screen shake command or state")]
    ScreenShake,
}

impl MusicaRuntimeError {
    /// Stable diagnostic identity, independent of display text and source operands.
    pub fn diagnostic_code(&self) -> &'static str {
        match self {
            Self::State => "ASTRA_EMU_MUSICA_RUNTIME_STATE",
            Self::ProgramCounter => "ASTRA_EMU_MUSICA_RUNTIME_PC",
            Self::Label => "ASTRA_EMU_MUSICA_RUNTIME_LABEL",
            Self::Operand => "ASTRA_EMU_MUSICA_RUNTIME_OPERAND",
            Self::UnsupportedOpcode { .. } => "ASTRA_EMU_MUSICA_RUNTIME_OPCODE",
            Self::NonYieldingCycle => "ASTRA_EMU_MUSICA_RUNTIME_NON_YIELDING_CYCLE",
            Self::Waiting => "ASTRA_EMU_MUSICA_RUNTIME_WAIT",
            Self::Overflow => "ASTRA_EMU_MUSICA_RUNTIME_OVERFLOW",
            Self::NativeSaveFormat => "ASTRA_EMU_MUSICA_NATIVE_SAVE_FORMAT",
            Self::ChainTarget => "ASTRA_EMU_MUSICA_RUNTIME_CHAIN",
            Self::AudioResource => "ASTRA_EMU_MUSICA_RUNTIME_AUDIO_RESOURCE",
            Self::Effect => "ASTRA_EMU_MUSICA_RUNTIME_EFFECT",
            Self::ScreenShake => "ASTRA_EMU_MUSICA_RUNTIME_SCREEN_SHAKE",
            Self::Pragma => "ASTRA_EMU_MUSICA_RUNTIME_PRAGMA",
            Self::Panel => "ASTRA_EMU_MUSICA_RUNTIME_PANEL",
        }
    }
}
