use astra_core::Hash256;
use astra_media_core::{
    DrawCommand, HeadlessRenderer, HeadlessRendererProvider, MediaError, RenderTargetFormat,
    Renderer2DProvider, RendererCreateRequest,
};
use astra_player_core::{
    PlayerHostCommand, PlayerHostCommandBatch, PlayerHostCommandError, PlayerHostResourceId,
};
use astra_vn_core::{
    CompiledStory, VnError, VnPlayerCommand, VnRunConfig, VnRuntime, VnStepOutput,
};

pub struct NativeVnHostCommandSource {
    runtime: VnRuntime,
    renderer: HeadlessRenderer,
    story_id: String,
    state_id: String,
    surface: PlayerHostResourceId,
    command_sequence: u64,
    width: u32,
}

impl NativeVnHostCommandSource {
    pub fn new(
        compiled: CompiledStory,
        config: VnRunConfig,
        width: u32,
        height: u32,
        surface: PlayerHostResourceId,
    ) -> Result<Self, NativeVnHostError> {
        let entry = compiled
            .story_manifest
            .stories
            .first()
            .ok_or(NativeVnHostError::EmptyStory)?;
        let state_id = entry
            .states
            .first()
            .cloned()
            .ok_or(NativeVnHostError::EmptyStory)?;
        let story_id = entry.id.clone();
        let runtime = VnRuntime::new(compiled, config)?;
        let renderer = HeadlessRendererProvider.create(RendererCreateRequest {
            width,
            height,
            format: RenderTargetFormat::Rgba8Srgb,
            profile: "player".to_string(),
        })?;
        Ok(Self {
            runtime,
            renderer,
            story_id,
            state_id,
            surface,
            command_sequence: 0,
            width,
        })
    }

    pub fn launch(&mut self) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let output = self.runtime.apply(VnPlayerCommand::Launch {
            story_id: self.story_id.clone(),
            state_id: self.state_id.clone(),
        })?;
        self.render(output)
    }

    pub fn advance(&mut self) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let output = self.runtime.apply(VnPlayerCommand::Advance)?;
        self.render(output)
    }

    pub fn choose(
        &mut self,
        option_id: impl Into<String>,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let output = self.runtime.apply(VnPlayerCommand::Choose {
            option_id: option_id.into(),
        })?;
        self.render(output)
    }

    pub fn primary_input(&mut self) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        if let Some(option_id) = self
            .runtime
            .state()
            .pending_choice
            .as_ref()
            .and_then(|choice| choice.options.first())
            .map(|option| option.id.clone())
        {
            self.choose(option_id)
        } else {
            self.advance()
        }
    }

    fn render(
        &mut self,
        output: VnStepOutput,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let state = output.state_hash_after_advance.as_bytes();
        let mut draw = vec![DrawCommand::clear([state[0], state[1], state[2], 255])];
        let bar_width = self.width.saturating_sub(40).max(1);
        for (index, presentation) in output.presentation.iter().enumerate() {
            let payload = serde_json::to_vec(presentation).map_err(NativeVnHostError::Serialize)?;
            let hash = Hash256::from_sha256(&payload);
            draw.push(DrawCommand::rect(
                format!("vn.presentation.{index}"),
                20,
                20 + index as u32 * 28,
                bar_width,
                20,
                [
                    hash.as_bytes()[0].max(24),
                    hash.as_bytes()[1].max(24),
                    hash.as_bytes()[2].max(24),
                    255,
                ],
            ));
        }
        let frame = self.renderer.capture_frame(&draw)?;
        self.command_sequence = self
            .command_sequence
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::PresentRgba {
                sequence: self.command_sequence,
                surface: self.surface,
                width: frame.width,
                height: frame.height,
                rgba8: frame.bytes,
            },
        ])?)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NativeVnHostError {
    #[error("compiled story has no playable entry state")]
    EmptyStory,
    #[error("Player host command sequence overflowed")]
    SequenceOverflow,
    #[error(transparent)]
    Vn(#[from] VnError),
    #[error(transparent)]
    Media(#[from] MediaError),
    #[error(transparent)]
    Command(#[from] PlayerHostCommandError),
    #[error("presentation serialization failed: {0}")]
    Serialize(serde_json::Error),
}
