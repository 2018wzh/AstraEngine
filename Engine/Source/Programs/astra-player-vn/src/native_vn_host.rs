use std::collections::BTreeMap;

use astra_asset::{AssetCatalog, VfsManifest, VfsSourceRef};
use astra_core::{Hash256, SchemaVersion};
use astra_media_core::{
    BlendMode, DrawCommand, GlyphBitmap, HeadlessRenderer, HeadlessRendererProvider, MediaError,
    RectI, RenderTargetFormat, Renderer2DProvider, RendererCreateRequest, TextureFrame,
};
use astra_player_core::{
    PlayerAction, PlayerHostCommand, PlayerHostCommandBatch, PlayerHostCommandError,
    PlayerHostResourceId,
};
use astra_plugin::{ProductRuntimeHost, RuntimeHostError, RuntimeHostSchemaRegistry};
use astra_plugin_abi::{
    GameRuntimeSessionId, RuntimeOpenRequest, RuntimeOutputDomain, RuntimeSectionCodec,
    RuntimeSectionPayload, RuntimeStepInput,
};
use astra_vn_core::{
    CompiledStory, PresentationCommand, VnPlayerCommand, VnRunConfig, VnRuntimeState,
};
use astra_vn_package::decode_compiled_story;
use astra_vn_runtime_provider::NativeVnRuntimeProvider;
use astra_vn_system::{SystemUiAction, SystemUiModel};

pub struct NativeVnHostCommandSource {
    host: ProductRuntimeHost,
    session_id: GameRuntimeSessionId,
    runtime_state: Option<VnRuntimeState>,
    renderer: HeadlessRenderer,
    surface: PlayerHostResourceId,
    command_sequence: u64,
    fixed_step: u64,
    width: u32,
    height: u32,
    textures: BTreeMap<String, TextureFrame>,
    scene_draw: Vec<DrawCommand>,
}

impl NativeVnHostCommandSource {
    pub fn new(
        compiled: CompiledStory,
        config: VnRunConfig,
        width: u32,
        height: u32,
        surface: PlayerHostResourceId,
    ) -> Result<Self, NativeVnHostError> {
        Self::open(compiled, config, width, height, surface, BTreeMap::new())
    }

    pub fn from_package(
        package: &astra_package::PackageReader,
        config: VnRunConfig,
        width: u32,
        height: u32,
        surface: PlayerHostResourceId,
    ) -> Result<Self, NativeVnHostError> {
        let compiled = decode_compiled_story(package)
            .map_err(|err| NativeVnHostError::Package(err.to_string()))?;
        let textures = load_package_textures(package)?;
        Self::open(compiled, config, width, height, surface, textures)
    }

    fn open(
        compiled: CompiledStory,
        config: VnRunConfig,
        width: u32,
        height: u32,
        surface: PlayerHostResourceId,
        textures: BTreeMap<String, TextureFrame>,
    ) -> Result<Self, NativeVnHostError> {
        if compiled.story_manifest.stories.is_empty() {
            return Err(NativeVnHostError::EmptyStory);
        }
        let package_hash = compiled.story_hash.to_string();
        let compiled_bytes = postcard::to_allocvec(&compiled)
            .map_err(|err| NativeVnHostError::Serialize(err.to_string()))?;
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
            "astra-player.native-vn",
            NativeVnRuntimeProvider::default(),
            schemas,
        )?;
        let open = host.open(RuntimeOpenRequest {
            target_id: "nativevn-game".to_string(),
            profile: config.profile,
            locale: config.locale,
            seed: 0,
            package_hash,
            sections: vec![compiled_section],
        })?;
        let renderer = HeadlessRendererProvider.create(RendererCreateRequest {
            width,
            height,
            format: RenderTargetFormat::Rgba8Srgb,
            profile: "player".to_string(),
        })?;
        tracing::info!(
            event = "player.vn.runtime.open",
            width,
            height,
            "opened AstraVN Player command source through ProductRuntimeHost"
        );
        Ok(Self {
            host,
            session_id: open.session_id,
            runtime_state: None,
            renderer,
            surface,
            command_sequence: 0,
            fixed_step: 0,
            width,
            height,
            textures,
            scene_draw: Vec::new(),
        })
    }

    pub fn launch(&mut self) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        self.step("launch_default", serde_json::json!({}))
    }

    pub fn dispatch_action(
        &mut self,
        action: PlayerAction,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let command = match action {
            PlayerAction::Advance => {
                if self
                    .runtime_state
                    .as_ref()
                    .is_some_and(|state| state.pending_choice.is_some())
                {
                    return Err(NativeVnHostError::Input(
                        "ASTRA_PLAYER_CHOICE_REQUIRED: advance cannot select a choice".into(),
                    ));
                }
                VnPlayerCommand::Advance
            }
            PlayerAction::ChooseIndex { index } => {
                let option_id = self
                    .runtime_state
                    .as_ref()
                    .and_then(|state| state.pending_choice.as_ref())
                    .and_then(|choice| choice.options.get(index))
                    .map(|option| option.id.clone())
                    .ok_or_else(|| {
                        NativeVnHostError::Input(
                            "ASTRA_PLAYER_CHOICE_INDEX: choice index is unavailable".into(),
                        )
                    })?;
                VnPlayerCommand::Choose { option_id }
            }
            PlayerAction::OpenSystemPage { page } => VnPlayerCommand::OpenSystem {
                page: astra_vn_core::SystemPageKind::parse(&page),
            },
            PlayerAction::Back => VnPlayerCommand::ReturnSystem,
        };
        self.command(command)
    }

    pub fn dispatch_pointer(
        &mut self,
        x: f64,
        y: f64,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let state = self.runtime_state.as_ref().ok_or_else(|| {
            NativeVnHostError::Input("ASTRA_PLAYER_STATE: runtime has not launched".into())
        })?;
        let model = if let Some(choice) = &state.pending_choice {
            SystemUiModel::choice(self.width, self.height, choice.options.len())
        } else if let Some(frame) = state.system_stack.last() {
            SystemUiModel::system(frame.page, self.width, self.height).ok_or_else(|| {
                NativeVnHostError::Input("ASTRA_PLAYER_SYSTEM_PAGE: unknown system page".into())
            })?
        } else {
            SystemUiModel::message(self.width, self.height)
        };
        let action = model.hit_test(x, y).cloned().ok_or_else(|| {
            NativeVnHostError::Input("ASTRA_PLAYER_HIT_TEST: pointer did not hit a control".into())
        })?;
        let action = match action {
            SystemUiAction::Advance => PlayerAction::Advance,
            SystemUiAction::ChooseIndex { index } => PlayerAction::ChooseIndex { index },
            SystemUiAction::Open { surface } => PlayerAction::OpenSystemPage {
                page: format!("{surface:?}").to_lowercase(),
            },
            SystemUiAction::Back => PlayerAction::Back,
            SystemUiAction::Activate { control_id } => {
                return Err(NativeVnHostError::Input(format!(
                    "ASTRA_PLAYER_CONTROL_UNBOUND: {control_id}"
                )))
            }
        };
        self.dispatch_action(action)
    }

    pub fn shutdown(mut self) -> Result<(), NativeVnHostError> {
        self.host.shutdown()?;
        self.host.destroy()?;
        Ok(())
    }

    fn command(
        &mut self,
        command: VnPlayerCommand,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let payload = serde_json::to_value(command)
            .map_err(|err| NativeVnHostError::Serialize(err.to_string()))?;
        self.step("command", payload)
    }

    fn step(
        &mut self,
        action: &str,
        payload: serde_json::Value,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        self.fixed_step = self
            .fixed_step
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        let output = self.host.step(RuntimeStepInput {
            session_id: self.session_id.clone(),
            fixed_step: self.fixed_step,
            action: action.to_string(),
            payload,
        })?;
        for trace in output
            .outputs
            .iter()
            .filter(|envelope| envelope.domain == RuntimeOutputDomain::Trace)
        {
            if trace.schema == "astra.vn.runtime_state.v1" {
                self.runtime_state = Some(
                    trace
                        .decode_postcard(
                            RuntimeOutputDomain::Trace,
                            "astra.vn.runtime_state.v1",
                            SchemaVersion::new(1, 0, 0),
                        )
                        .map_err(|err| NativeVnHostError::Serialize(err.to_string()))?,
                );
            }
        }
        let presentation = output
            .outputs
            .iter()
            .filter(|envelope| envelope.domain == RuntimeOutputDomain::Presentation)
            .map(|envelope| {
                envelope
                    .decode_postcard(
                        RuntimeOutputDomain::Presentation,
                        "astra.vn.presentation_command.v1",
                        SchemaVersion::new(1, 0, 0),
                    )
                    .map_err(|err| NativeVnHostError::Serialize(err.to_string()))
            })
            .collect::<Result<Vec<PresentationCommand>, _>>()?;
        self.render(&presentation)
    }

    fn render(
        &mut self,
        presentation: &[PresentationCommand],
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let mut draw = vec![DrawCommand::clear([8, 10, 16, 255])];
        for command in presentation {
            match command {
                PresentationCommand::Dialogue { key, speaker, .. } => {
                    let panel_height = (self.height / 3).max(64);
                    draw.push(DrawCommand::rect(
                        "vn.dialogue.panel",
                        24,
                        self.height.saturating_sub(panel_height + 24),
                        self.width.saturating_sub(48),
                        panel_height,
                        [18, 22, 34, 236],
                    ));
                    if let Some(speaker) = speaker {
                        push_bitmap_text(
                            &mut draw,
                            speaker,
                            42,
                            self.height.saturating_sub(panel_height + 4),
                            [120, 210, 255, 255],
                        );
                    }
                    push_bitmap_text(
                        &mut draw,
                        key,
                        42,
                        self.height.saturating_sub(panel_height - 28),
                        [245, 245, 248, 255],
                    );
                }
                PresentationCommand::Choice { key, options } => {
                    push_bitmap_text(&mut draw, key, 42, 32, [245, 245, 248, 255]);
                    for (index, option) in options.iter().enumerate() {
                        let y = 64 + index as u32 * 38;
                        draw.push(DrawCommand::rect(
                            format!("vn.choice.{}", option.id),
                            34,
                            y,
                            self.width.saturating_sub(68),
                            30,
                            [30, 48, 70, 245],
                        ));
                        push_bitmap_text(&mut draw, &option.key, 46, y + 8, [255, 255, 255, 255]);
                    }
                }
                PresentationCommand::SystemPage { page } => {
                    push_bitmap_text(
                        &mut draw,
                        &format!("SYSTEM {page:?}"),
                        42,
                        42,
                        [220, 230, 255, 255],
                    );
                }
                PresentationCommand::Stage {
                    command,
                    attributes,
                } => {
                    self.apply_stage_command(command, attributes)?;
                }
                PresentationCommand::Marker { .. } => {}
            }
        }
        draw.splice(1..1, self.scene_draw.clone());
        let frame = self.renderer.capture_frame(&draw)?;
        self.command_sequence = self
            .command_sequence
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        tracing::trace!(
            event = "player.vn.runtime.command.emit",
            sequence = self.command_sequence,
            presentation_count = presentation.len(),
            frame_hash = %frame.hash,
            "emitted AstraVN Player host command"
        );
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

    fn apply_stage_command(
        &mut self,
        command: &str,
        attributes: &BTreeMap<String, String>,
    ) -> Result<(), NativeVnHostError> {
        match command {
            "background" => {
                let asset_id = required_attribute(attributes, "asset", command)?;
                let frame = self.texture(asset_id)?.clone();
                self.scene_draw
                    .retain(|draw| !draw_id_is(draw, "vn.scene.background"));
                self.scene_draw.insert(
                    0,
                    DrawCommand::Texture {
                        id: "vn.scene.background".to_string(),
                        frame,
                        destination: RectI::new(0, 0, self.width, self.height),
                        opacity: 1.0,
                        blend: BlendMode::Alpha,
                    },
                );
            }
            "show" => {
                let asset_id = required_attribute(attributes, "asset", command)?;
                let frame = self.texture(asset_id)?.clone();
                let destination_height = self.height.saturating_mul(9) / 10;
                let destination_width = ((destination_height as u64 * frame.width as u64)
                    / frame.height as u64)
                    .min(self.width as u64) as u32;
                let x = match attributes.get("at").map(String::as_str) {
                    Some("left") => self.width / 12,
                    Some("center") => self.width.saturating_sub(destination_width) / 2,
                    _ => self
                        .width
                        .saturating_sub(destination_width + self.width / 12),
                };
                let id = attributes
                    .get("id")
                    .cloned()
                    .or_else(|| attributes.get("character").cloned())
                    .unwrap_or_else(|| "character".to_string());
                let draw_id = format!("vn.scene.character.{id}");
                self.scene_draw.retain(|draw| !draw_id_is(draw, &draw_id));
                self.scene_draw.push(DrawCommand::Texture {
                    id: draw_id,
                    frame,
                    destination: RectI::new(
                        x as i32,
                        self.height.saturating_sub(destination_height) as i32,
                        destination_width,
                        destination_height,
                    ),
                    opacity: 1.0,
                    blend: BlendMode::Alpha,
                });
            }
            "hide" => {
                let id = attributes
                    .get("id")
                    .or_else(|| attributes.get("character"))
                    .ok_or_else(|| {
                        NativeVnHostError::Asset(
                            "ASTRA_PLAYER_STAGE_ATTRIBUTE: hide requires id or character"
                                .to_string(),
                        )
                    })?;
                let draw_id = format!("vn.scene.character.{id}");
                self.scene_draw.retain(|draw| !draw_id_is(draw, &draw_id));
            }
            "movie" => {
                let asset_id = required_attribute(attributes, "asset", command)?;
                return Err(NativeVnHostError::Asset(format!(
                    "ASTRA_PLAYER_VIDEO_DECODE_REQUIRED: packaged video asset {asset_id} requires the platform decode bridge"
                )));
            }
            _ => {}
        }
        Ok(())
    }

    fn texture(&self, asset_id: &str) -> Result<&TextureFrame, NativeVnHostError> {
        self.textures.get(asset_id).ok_or_else(|| {
            NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_ASSET_MISSING: cooked texture {asset_id} is not mounted"
            ))
        })
    }
}

fn load_package_textures(
    package: &astra_package::PackageReader,
) -> Result<BTreeMap<String, TextureFrame>, NativeVnHostError> {
    let catalog: AssetCatalog = serde_json::from_slice(
        &package
            .container()
            .read_section("asset.catalog")
            .map_err(|err| NativeVnHostError::Package(err.to_string()))?,
    )
    .map_err(|err| NativeVnHostError::Package(err.to_string()))?;
    let manifest: VfsManifest = serde_json::from_slice(
        &package
            .container()
            .read_section("asset.vfs_manifest")
            .map_err(|err| NativeVnHostError::Package(err.to_string()))?,
    )
    .map_err(|err| NativeVnHostError::Package(err.to_string()))?;
    let mut textures = BTreeMap::new();
    for asset in catalog.assets {
        if !asset.media_kind.starts_with("image") {
            continue;
        }
        let entry = manifest
            .entries
            .iter()
            .find(|entry| entry.uri == asset.uri)
            .ok_or_else(|| {
                NativeVnHostError::Asset(format!(
                    "ASTRA_PLAYER_ASSET_VFS_MISSING: catalog asset {} has no VFS entry",
                    asset.asset_id
                ))
            })?;
        let VfsSourceRef::PackageSection { section_id } = &entry.source else {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_ASSET_SOURCE: asset {} is not package-backed",
                asset.asset_id
            )));
        };
        let encoded = package
            .container()
            .read_section(section_id)
            .map_err(|err| NativeVnHostError::Package(err.to_string()))?;
        if Hash256::from_sha256(&encoded) != entry.hash {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_ASSET_HASH: asset {} failed VFS hash validation",
                asset.asset_id
            )));
        }
        let decoded = image::load_from_memory(&encoded)
            .map_err(|err| NativeVnHostError::Asset(format!("ASTRA_PLAYER_ASSET_DECODE: {err}")))?
            .to_rgba8();
        let (width, height) = decoded.dimensions();
        let rgba8 = decoded.into_raw();
        textures.insert(
            asset.asset_id,
            TextureFrame {
                width,
                height,
                hash: Hash256::from_sha256(&rgba8),
                rgba8,
            },
        );
    }
    Ok(textures)
}

fn required_attribute<'a>(
    attributes: &'a BTreeMap<String, String>,
    key: &str,
    command: &str,
) -> Result<&'a str, NativeVnHostError> {
    attributes.get(key).map(String::as_str).ok_or_else(|| {
        NativeVnHostError::Asset(format!(
            "ASTRA_PLAYER_STAGE_ATTRIBUTE: {command} requires {key}"
        ))
    })
}

fn draw_id_is(draw: &DrawCommand, expected: &str) -> bool {
    match draw {
        DrawCommand::Rect { id, .. }
        | DrawCommand::Texture { id, .. }
        | DrawCommand::VideoFrame { id, .. }
        | DrawCommand::Glyph { id, .. } => id == expected,
        _ => false,
    }
}

fn push_bitmap_text(
    commands: &mut Vec<DrawCommand>,
    text: &str,
    mut x: u32,
    y: u32,
    rgba: [u8; 4],
) {
    for (index, character) in text.chars().take(96).enumerate() {
        let alpha8 = bitmap_glyph(character);
        let hash = Hash256::from_sha256(&alpha8);
        commands.push(DrawCommand::Glyph {
            id: format!("vn.text.{index}"),
            glyph: GlyphBitmap {
                width: 5,
                height: 7,
                alpha8,
                hash,
            },
            x: x as i32,
            y: y as i32,
            rgba,
            opacity: 1.0,
            blend: BlendMode::Alpha,
        });
        x = x.saturating_add(6);
    }
}

fn bitmap_glyph(character: char) -> Vec<u8> {
    let rows: [u8; 7] = match character.to_ascii_uppercase() {
        'A' => [0x0e, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        'B' => [0x1e, 0x11, 0x11, 0x1e, 0x11, 0x11, 0x1e],
        'C' => [0x0f, 0x10, 0x10, 0x10, 0x10, 0x10, 0x0f],
        'D' => [0x1e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1e],
        'E' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x1f],
        'F' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x10],
        'G' => [0x0f, 0x10, 0x10, 0x13, 0x11, 0x11, 0x0f],
        'H' => [0x11, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        'I' => [0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x1f],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x12, 0x12, 0x0c],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1f],
        'M' => [0x11, 0x1b, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x19, 0x15, 0x13, 0x13, 0x11],
        'O' => [0x0e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        'P' => [0x1e, 0x11, 0x11, 0x1e, 0x10, 0x10, 0x10],
        'Q' => [0x0e, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0d],
        'R' => [0x1e, 0x11, 0x11, 0x1e, 0x14, 0x12, 0x11],
        'S' => [0x0f, 0x10, 0x10, 0x0e, 0x01, 0x01, 0x1e],
        'T' => [0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0a, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0a],
        'X' => [0x11, 0x11, 0x0a, 0x04, 0x0a, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0a, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1f],
        '0' => [0x0e, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0e],
        '1' => [0x04, 0x0c, 0x04, 0x04, 0x04, 0x04, 0x0e],
        '2' => [0x0e, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1f],
        '3' => [0x1e, 0x01, 0x01, 0x0e, 0x01, 0x01, 0x1e],
        '4' => [0x02, 0x06, 0x0a, 0x12, 0x1f, 0x02, 0x02],
        '5' => [0x1f, 0x10, 0x10, 0x1e, 0x01, 0x01, 0x1e],
        '6' => [0x0e, 0x10, 0x10, 0x1e, 0x11, 0x11, 0x0e],
        '7' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0e, 0x11, 0x11, 0x0e, 0x11, 0x11, 0x0e],
        '9' => [0x0e, 0x11, 0x11, 0x0f, 0x01, 0x01, 0x0e],
        '.' => [0, 0, 0, 0, 0, 0x06, 0x06],
        '_' => [0, 0, 0, 0, 0, 0, 0x1f],
        '-' => [0, 0, 0, 0x1f, 0, 0, 0],
        ' ' => [0; 7],
        _ => [0x0e, 0x11, 0x01, 0x02, 0x04, 0, 0x04],
    };
    rows.into_iter()
        .flat_map(|row| {
            (0..5)
                .rev()
                .map(move |bit| if row & (1 << bit) != 0 { 255 } else { 0 })
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum NativeVnHostError {
    #[error("compiled story has no playable entry state")]
    EmptyStory,
    #[error("Player host command sequence overflowed")]
    SequenceOverflow,
    #[error(transparent)]
    RuntimeHost(#[from] RuntimeHostError),
    #[error(transparent)]
    Media(#[from] MediaError),
    #[error(transparent)]
    Command(#[from] PlayerHostCommandError),
    #[error("presentation serialization failed: {0}")]
    Serialize(String),
    #[error("package validation failed: {0}")]
    Package(String),
    #[error("presentation asset failed: {0}")]
    Asset(String),
    #[error("player input failed: {0}")]
    Input(String),
}
