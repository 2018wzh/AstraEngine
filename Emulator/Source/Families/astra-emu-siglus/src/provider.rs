use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use astra_byte_source::OwnedByteBuffer;
use astra_core::Hash256;
use astra_emu_family_api::*;
use siglus_hosted::hosted_port::{HostedCaseProfile, HostedResourcePort};
use siglus_hosted::hosted_session::{
    HostedDecodeMaterialProfile, HostedIngress, HostedIngressKind, HostedOpenConfig, HostedSession,
    HostedStepRequest, HostedTextureDelta, HostedWaitState,
};
use siglus_hosted::layer::{
    ClipRect, RenderSprite, Sprite, SpriteBlend, SpriteFit, SpriteSizeMode,
};
use siglus_hosted::runtime::input::{VmKey, VmMouseButton};

use crate::{
    SiglusPrivateMaterialPortV8, SiglusResourcePortV8, SIGLUS_FAMILY_ID, SIGLUS_PROVIDER_ID,
};

const MAX_GAMEEXE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RESOURCE_BYTES: u64 = 512 * 1024 * 1024;

struct SiglusSession {
    hosted: HostedSession,
    resources: Arc<dyn HostedResourcePort>,
    seed: u64,
    fixed_delta_ns: u64,
    last_step: u64,
    next_live_sequence: u64,
    state_revision: u64,
    pointer_x: f64,
    pointer_y: f64,
    poisoned: bool,
    textures: BTreeMap<u32, RetainedTexture>,
}

#[derive(Clone, Copy)]
struct RetainedTexture {
    generation: u64,
    width: u32,
    height: u32,
}

pub struct SiglusRuntimeProvider {
    vfs: Arc<dyn LegacyVfsReader>,
    private_material: Option<Arc<dyn LegacyPrivateMaterialHostV8>>,
    sessions: BTreeMap<String, SiglusSession>,
}

impl SiglusRuntimeProvider {
    pub fn new(
        vfs: Arc<dyn LegacyVfsReader>,
        private_material: Option<Arc<dyn LegacyPrivateMaterialHostV8>>,
    ) -> Self {
        Self {
            vfs,
            private_material,
            sessions: BTreeMap::new(),
        }
    }

    pub fn has_active_sessions(&self) -> bool {
        !self.sessions.is_empty()
    }
}

pub fn create_static_siglus_provider(
    vfs: Arc<dyn LegacyVfsReader>,
    private_material: Option<Arc<dyn LegacyPrivateMaterialHostV8>>,
) -> Result<Box<dyn LegacyRuntimeProvider>, LegacyProviderError> {
    let provider = SiglusRuntimeProvider::new(vfs, private_material);
    provider.descriptor().validate()?;
    Ok(Box::new(provider))
}

impl LegacyRuntimeProvider for SiglusRuntimeProvider {
    fn descriptor(&self) -> LegacyFamilyPluginDescriptor {
        siglus_descriptor()
    }

    fn probe(
        &self,
        ctx: &LegacyRuntimeHostCtx,
        request: LegacyProbeRequest,
    ) -> Result<LegacyProbeReport, LegacyProviderError> {
        ctx.validate()?;
        if request.max_entries == 0 || request.max_metadata_bytes == 0 {
            return Err(invalid(
                "ASTRA_SIGLUS_PROBE_BUDGET",
                "probe budget is empty",
            ));
        }
        let mut gameexe = Vec::new();
        let mut scenes = Vec::new();
        for uri in request
            .candidate_uris
            .iter()
            .take(request.max_entries as usize)
        {
            let lower = uri.to_ascii_lowercase();
            if matches!(lower.as_str(), "gameexe.dat" | "gameexe.chs")
                || lower.ends_with("/gameexe.dat")
                || lower.ends_with("/gameexe.chs")
            {
                gameexe.push(uri.clone());
            } else if matches!(lower.as_str(), "scene.pck" | "scene.chs")
                || lower.ends_with("/scene.pck")
                || lower.ends_with("/scene.chs")
            {
                scenes.push(uri.clone());
            }
        }
        if gameexe.len() != 1 || scenes.len() != 1 {
            return Err(invalid(
                "ASTRA_SIGLUS_PROBE_AMBIGUOUS",
                "probe requires exactly one explicitly selected Gameexe and Scene pair",
            ));
        }
        let gameexe_bytes = self.vfs.read_file(
            &request.root_mount_id,
            &gameexe[0],
            request.max_metadata_bytes.min(MAX_GAMEEXE_BYTES),
        )?;
        let identity = Hash256::from_sha256(gameexe_bytes.as_slice());
        Ok(LegacyProbeReport {
            family_id: FamilyId(SIGLUS_FAMILY_ID.into()),
            confidence_permyriad: 10_000,
            markers: vec!["siglus.gameexe".into(), "siglus.scene_pck".into()],
            blockers: Vec::new(),
            content_identity: identity,
        })
    }

    fn open(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        request: LegacyOpenRequest,
    ) -> Result<LegacyRuntimeSessionId, LegacyProviderError> {
        ctx.validate()?;
        validate_symbol("session_id", &request.requested_session_id.0)?;
        if request.fixed_delta_ns == 0 || request.fixed_delta_ns > 1_000_000_000 {
            return Err(invalid(
                "ASTRA_SIGLUS_FIXED_DELTA",
                "fixed delta is invalid",
            ));
        }
        if self.sessions.contains_key(&request.requested_session_id.0) {
            return Err(invalid(
                "ASTRA_SIGLUS_SESSION_DUPLICATE",
                "session is already active",
            ));
        }
        let (root_uri, profile) = parse_open_binding(&request)?;
        let resources: Arc<dyn HostedResourcePort> = Arc::new(
            SiglusResourcePortV8::new(self.vfs.clone(), ctx.mount_set_id.clone(), root_uri)
                .map_err(|_| invalid("ASTRA_SIGLUS_VFS_BINDING", "resource binding failed"))?,
        );
        let gameexe_name = match profile {
            HostedCaseProfile::Original => "Gameexe.dat",
            HostedCaseProfile::SimplifiedChinese => "Gameexe.chs",
        };
        let gameexe = resources
            .stat(gameexe_name)
            .and_then(|stat| {
                let len = usize::try_from(stat.byte_len)
                    .map_err(|_| anyhow::anyhow!("ASTRA_SIGLUS_GAMEEXE_BOUNDS"))?;
                resources.read_range(
                    gameexe_name,
                    stat.revision,
                    0,
                    len,
                    MAX_GAMEEXE_BYTES as usize,
                )
            })
            .map_err(|_| invalid("ASTRA_SIGLUS_GAMEEXE_READ", "Gameexe read failed"))?;
        if Hash256::from_sha256(&gameexe) != request.case_fingerprint {
            return Err(invalid(
                "ASTRA_SIGLUS_CASE_FINGERPRINT",
                "Gameexe fingerprint does not match the probed case",
            ));
        }
        let decode_material = match request.family_options.get("siglus.private_material_id") {
            Some(secret_id) => HostedDecodeMaterialProfile::PrivateExeKey16 {
                secret_id: secret_id.clone(),
            },
            None => HostedDecodeMaterialProfile::Public,
        };
        let private_port = self.private_material.as_ref().map(|host| {
            Arc::new(SiglusPrivateMaterialPortV8::new(
                host.clone(),
                request.requested_session_id.clone(),
            )) as Arc<dyn siglus_hosted::hosted_port::HostedPrivateMaterialPort>
        });
        let hosted = catch_unwind(AssertUnwindSafe(|| {
            HostedSession::open(
                HostedOpenConfig {
                    profile,
                    decode_material,
                    ..HostedOpenConfig::default()
                },
                resources.clone(),
                private_port,
            )
        }))
        .map_err(|_| invalid("ASTRA_SIGLUS_OPEN_PANIC", "hosted open panicked"))?
        .map_err(|_| invalid("ASTRA_SIGLUS_OPEN_FAILED", "hosted open failed"))?;
        let session = SiglusSession {
            hosted,
            resources,
            seed: request.session_seed,
            fixed_delta_ns: request.fixed_delta_ns,
            last_step: 0,
            next_live_sequence: 1,
            state_revision: 0,
            pointer_x: 0.0,
            pointer_y: 0.0,
            poisoned: false,
            textures: BTreeMap::new(),
        };
        self.sessions
            .insert(request.requested_session_id.0.clone(), session);
        Ok(request.requested_session_id)
    }

    fn step(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        input: LegacyStepInput,
    ) -> Result<LegacyStepOutput, LegacyProviderError> {
        ctx.validate()?;
        input.validate()?;
        let session = self
            .sessions
            .get_mut(&session_id.0)
            .ok_or_else(|| invalid("ASTRA_SIGLUS_SESSION_MISSING", "session is not active"))?;
        if session.poisoned {
            return Err(invalid(
                "ASTRA_SIGLUS_SESSION_POISONED",
                "session is poisoned",
            ));
        }
        if input.tick_index != session.last_step + 1
            || input.session_seed != session.seed
            || input.delta_ns != session.fixed_delta_ns
        {
            session.poisoned = true;
            return Err(invalid(
                "ASTRA_SIGLUS_STEP_IDENTITY",
                "step identity drifted",
            ));
        }
        if !input.await_results.is_empty() || !input.provider_results.is_empty() {
            session.poisoned = true;
            return Err(invalid(
                "ASTRA_SIGLUS_COMPLETION_UNSUPPORTED",
                "hosted completion channel is not implemented",
            ));
        }
        let ingress = map_ingress(session, input.input_edges)?;
        let hosted_step_index = input.tick_index - 1;
        let hosted_delta_ns = rational_frame_delta_ns(hosted_step_index, 60)?;
        let delta = match catch_unwind(AssertUnwindSafe(|| {
            session.hosted.step(HostedStepRequest {
                step_index: hosted_step_index,
                delta_ns: hosted_delta_ns,
                ingress,
            })
        })) {
            Ok(Ok(delta)) => delta,
            Ok(Err(_)) => {
                session.poisoned = true;
                return Err(invalid("ASTRA_SIGLUS_STEP_FAILED", "hosted step failed"));
            }
            Err(_) => {
                session.poisoned = true;
                return Err(invalid("ASTRA_SIGLUS_STEP_PANIC", "hosted step panicked"));
            }
        };
        let scene = translate_scene(&delta, session.next_live_sequence, &mut session.textures)
            .inspect_err(|_error| {
                session.poisoned = true;
            })?;
        let mut live = LegacyLiveOutput::default();
        let mut coverage = LegacyCoverageDelta::default();
        if let Some(scene) = scene {
            live.scenes.push(scene);
            coverage.presentation_commands = 1;
            session.next_live_sequence = session
                .next_live_sequence
                .checked_add(1)
                .ok_or_else(|| invalid("ASTRA_SIGLUS_SEQUENCE_OVERFLOW", "sequence overflowed"))?;
        }
        session.last_step = input.tick_index;
        session.state_revision = session
            .state_revision
            .checked_add(1)
            .ok_or_else(|| invalid("ASTRA_SIGLUS_REVISION_OVERFLOW", "revision overflowed"))?;
        let output = LegacyStepOutput {
            status: if delta.terminal {
                LegacyRuntimeStatus::Terminal
            } else if delta.wait == HostedWaitState::Blocked {
                LegacyRuntimeStatus::Awaiting
            } else {
                LegacyRuntimeStatus::Active
            },
            live,
            control: LegacyControlTransaction::default(),
            trace: Vec::new(),
            diagnostics: Vec::new(),
            coverage,
            state_revision: session.state_revision,
        };
        output.validate(&input.budget)?;
        Ok(output)
    }

    fn save(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
    ) -> Result<LegacySnapshotEnvelope, LegacyProviderError> {
        ctx.validate()?;
        require_session(&self.sessions, session)?;
        Err(invalid(
            "ASTRA_SIGLUS_SNAPSHOT_NOT_IMPLEMENTED",
            "Siglus hosted runtime snapshot is not implemented",
        ))
    }

    fn restore(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
        snapshot: &LegacySnapshotEnvelope,
    ) -> Result<LegacyRestoreReport, LegacyProviderError> {
        ctx.validate()?;
        snapshot.validate()?;
        require_session(&self.sessions, session)?;
        Err(invalid(
            "ASTRA_SIGLUS_SNAPSHOT_NOT_IMPLEMENTED",
            "Siglus hosted runtime restore is not implemented",
        ))
    }

    fn take_ephemeral_text(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
        lease_id: &str,
    ) -> Result<Option<LegacyEphemeralText>, LegacyProviderError> {
        ctx.validate()?;
        validate_symbol("lease_id", lease_id)?;
        require_session(&self.sessions, session)?;
        Ok(None)
    }

    fn read_session_resource(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        resource_uri: &str,
        max_bytes: u64,
    ) -> Result<OwnedByteBuffer, LegacyProviderError> {
        ctx.validate()?;
        if max_bytes == 0 || max_bytes > MAX_RESOURCE_BYTES {
            return Err(invalid(
                "ASTRA_SIGLUS_RESOURCE_BOUNDS",
                "resource bound is invalid",
            ));
        }
        let session = require_session(&self.sessions, session_id)?;
        let stat = session
            .resources
            .stat(resource_uri)
            .map_err(|_| invalid("ASTRA_SIGLUS_RESOURCE_STAT", "resource stat failed"))?;
        if stat.byte_len > max_bytes {
            return Err(invalid(
                "ASTRA_SIGLUS_RESOURCE_BOUNDS",
                "resource exceeds bound",
            ));
        }
        let len = usize::try_from(stat.byte_len)
            .map_err(|_| invalid("ASTRA_SIGLUS_RESOURCE_BOUNDS", "resource length overflowed"))?;
        session
            .resources
            .read_range(resource_uri, stat.revision, 0, len, len)
            .map(OwnedByteBuffer::from_vec)
            .map_err(|_| invalid("ASTRA_SIGLUS_RESOURCE_READ", "resource read failed"))
    }

    fn begin_session_resource_read(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session: &LegacyRuntimeSessionId,
        resource_uri: &str,
        max_bytes: u64,
    ) -> Result<LegacyResourceRead, LegacyProviderError> {
        let bytes = self.read_session_resource(ctx, session, resource_uri, max_bytes)?;
        LegacyResourceRead::spawn(move || Ok(bytes))
    }

    fn shutdown(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
    ) -> Result<LegacyShutdownReport, LegacyProviderError> {
        ctx.validate()?;
        let mut session = self
            .sessions
            .remove(&session_id.0)
            .ok_or_else(|| invalid("ASTRA_SIGLUS_SESSION_MISSING", "session is not active"))?;
        session
            .hosted
            .shutdown()
            .map_err(|_| invalid("ASTRA_SIGLUS_SHUTDOWN_FAILED", "hosted shutdown failed"))?;
        Ok(LegacyShutdownReport {
            final_state_revision: session.state_revision,
            instruction_count: session.last_step,
            syscall_count: 0,
            evidence_vm_trace: Vec::new(),
            diagnostics: Vec::new(),
        })
    }
}

pub(crate) fn siglus_descriptor() -> LegacyFamilyPluginDescriptor {
    LegacyFamilyPluginDescriptor {
        family_id: FamilyId(SIGLUS_FAMILY_ID.into()),
        plugin_id: "astra.emu.siglus".into(),
        provider_id: SIGLUS_PROVIDER_ID.into(),
        engine_version: env!("CARGO_PKG_VERSION").into(),
        rustc_fingerprint: env!("ASTRA_SIGLUS_RUSTC_FINGERPRINT").into(),
        feature_fingerprint: env!("ASTRA_SIGLUS_FEATURE_FINGERPRINT").into(),
        abi_fingerprint: LEGACY_FAMILY_ABI_FINGERPRINT.into(),
        supported_formats: vec![
            "siglus.gameexe".into(),
            "siglus.scene_pck".into(),
            "siglus.g00".into(),
            "siglus.nwa".into(),
            "siglus.ovk".into(),
            "siglus.omv".into(),
        ],
        permissions: vec![
            "vfs.read".into(),
            "media.submit".into(),
            "text.layout".into(),
            "private_material.read".into(),
            "save.atomic_write".into(),
        ],
        report_redaction: "astra.emu.redaction.v1".into(),
        license: "MPL-2.0".into(),
    }
}

fn parse_open_binding(
    request: &LegacyOpenRequest,
) -> Result<(String, HostedCaseProfile), LegacyProviderError> {
    let profile = match request.compatibility_profile.as_str() {
        "siglus.original" => HostedCaseProfile::Original,
        "siglus.translated" => HostedCaseProfile::SimplifiedChinese,
        _ => {
            return Err(invalid(
                "ASTRA_SIGLUS_PROFILE",
                "compatibility profile must be siglus.original or siglus.translated",
            ))
        }
    };
    let expected = match profile {
        HostedCaseProfile::Original => "Gameexe.dat",
        HostedCaseProfile::SimplifiedChinese => "Gameexe.chs",
    };
    let root = if request.script_uri == expected {
        ""
    } else {
        request
            .script_uri
            .strip_suffix(&format!("/{expected}"))
            .ok_or_else(|| {
                invalid(
                    "ASTRA_SIGLUS_SCRIPT_URI",
                    "script URI does not match profile",
                )
            })?
    };
    if root.contains('\\')
        || root.starts_with('/')
        || root.split('/').any(|part| matches!(part, "." | ".."))
    {
        return Err(invalid(
            "ASTRA_SIGLUS_SCRIPT_URI",
            "script URI root is invalid",
        ));
    }
    Ok((root.to_string(), profile))
}

fn map_ingress(
    session: &mut SiglusSession,
    edges: Vec<LegacyInputEdge>,
) -> Result<Vec<HostedIngress>, LegacyProviderError> {
    let mut ingress = Vec::with_capacity(edges.len());
    for edge in edges {
        let kind = match edge.control.as_str() {
            "pointer.x" => {
                session.pointer_x = f64::from(edge.value);
                HostedIngressKind::MouseMove {
                    x: session.pointer_x,
                    y: session.pointer_y,
                }
            }
            "pointer.y" => {
                session.pointer_y = f64::from(edge.value);
                HostedIngressKind::MouseMove {
                    x: session.pointer_x,
                    y: session.pointer_y,
                }
            }
            "pointer.primary" => HostedIngressKind::MouseButton {
                button: VmMouseButton::Left,
                down: edge.pressed,
            },
            "pointer.secondary" => HostedIngressKind::MouseButton {
                button: VmMouseButton::Right,
                down: edge.pressed,
            },
            "wheel" => HostedIngressKind::MouseWheel {
                delta_y: edge.value.round() as i32,
            },
            control => HostedIngressKind::Key {
                key: map_key(control)?,
                down: edge.pressed,
            },
        };
        ingress.push(HostedIngress {
            sequence: edge.sequence,
            kind,
        });
    }
    Ok(ingress)
}

fn map_key(control: &str) -> Result<VmKey, LegacyProviderError> {
    match control {
        "enter" => Ok(VmKey::Enter),
        "escape" => Ok(VmKey::Escape),
        "space" => Ok(VmKey::Space),
        "shift" => Ok(VmKey::Shift),
        "control" => Ok(VmKey::Control),
        "arrow_up" => Ok(VmKey::ArrowUp),
        "arrow_down" => Ok(VmKey::ArrowDown),
        "arrow_left" => Ok(VmKey::ArrowLeft),
        "arrow_right" => Ok(VmKey::ArrowRight),
        _ => Err(invalid(
            "ASTRA_SIGLUS_INPUT_KEY",
            "canonical key is unsupported",
        )),
    }
}

fn rational_frame_delta_ns(index: u64, rate: u64) -> Result<u64, LegacyProviderError> {
    let current = u128::from(index)
        .checked_mul(1_000_000_000)
        .ok_or_else(|| invalid("ASTRA_SIGLUS_CLOCK_OVERFLOW", "clock overflowed"))?
        / u128::from(rate);
    let next = u128::from(index + 1)
        .checked_mul(1_000_000_000)
        .ok_or_else(|| invalid("ASTRA_SIGLUS_CLOCK_OVERFLOW", "clock overflowed"))?
        / u128::from(rate);
    u64::try_from(next - current)
        .map_err(|_| invalid("ASTRA_SIGLUS_CLOCK_OVERFLOW", "clock overflowed"))
}

fn translate_scene(
    delta: &siglus_hosted::hosted_session::HostedDelta,
    sequence: u64,
    retained_textures: &mut BTreeMap<u32, RetainedTexture>,
) -> Result<Option<LegacySceneTransactionV8>, LegacyProviderError> {
    let Some(frame) = delta.frame.as_ref() else {
        if !delta.textures.is_empty() {
            return Err(invalid(
                "ASTRA_SIGLUS_SCENE_BOUNDARY",
                "texture delta was emitted without a presentation frame",
            ));
        }
        return Ok(None);
    };
    if frame.wipe.is_some() {
        return Err(invalid(
            "ASTRA_SIGLUS_WIPE_UNIMPLEMENTED",
            "Siglus wipe lowering is not implemented",
        ));
    }
    if delta.reset_render_resources {
        retained_textures.clear();
    }
    let mut resources = Vec::new();
    for texture in &delta.textures {
        append_texture(texture, retained_textures, &mut resources)?;
    }
    let dimensions = (delta.width, delta.height);
    if dimensions.0 == 0 || dimensions.1 == 0 {
        return Err(invalid(
            "ASTRA_SIGLUS_STAGE_DIMENSIONS",
            "hosted delta stage dimensions are invalid",
        ));
    }
    let texture_sizes = retained_textures
        .iter()
        .map(|(&image_id, texture)| (image_id, (texture.width, texture.height)))
        .collect::<BTreeMap<_, _>>();
    let mut draws = Vec::new();
    for render_sprite in &frame.sprites {
        if let Some(draw) = translate_sprite(render_sprite, dimensions, &texture_sizes)? {
            draws.push(draw);
        }
    }
    let transaction = LegacySceneTransactionV8 {
        sequence,
        width: dimensions.0,
        height: dimensions.1,
        compositing: LegacySceneCompositingV1::EncodedSrgb,
        resources,
        draws,
        mesh_batches: Vec::new(),
        text_draws: Vec::new(),
        effects: Vec::new(),
        reset_resources: delta.reset_render_resources,
    };
    transaction.validate()?;
    Ok(Some(transaction))
}

fn append_texture(
    texture: &HostedTextureDelta,
    retained_textures: &mut BTreeMap<u32, RetainedTexture>,
    output: &mut Vec<LegacySceneResourceOperationV8>,
) -> Result<(), LegacyProviderError> {
    if texture.image_id == u32::MAX || texture.generation == 0 {
        return Err(invalid(
            "ASTRA_SIGLUS_TEXTURE_IDENTITY",
            "texture identity is invalid",
        ));
    }
    let retained = RetainedTexture {
        generation: texture.generation,
        width: texture.width,
        height: texture.height,
    };
    if let Some(previous) = retained_textures.get(&texture.image_id).copied() {
        if texture.generation <= previous.generation {
            return Err(invalid(
                "ASTRA_SIGLUS_TEXTURE_GENERATION",
                "texture generation regressed",
            ));
        }
        output.push(LegacySceneResourceOperationV8::DestroyTexture {
            texture_id: texture.image_id,
            generation: previous.generation,
        });
    }
    retained_textures.insert(texture.image_id, retained);
    output.push(LegacySceneResourceOperationV8::CreateTexture {
        texture_id: texture.image_id,
        generation: texture.generation,
        width: texture.width,
        height: texture.height,
        format: LegacyTextureFormat::Rgba8,
        pixels: OwnedByteBuffer::from_vec(texture.rgba.clone()),
    });
    Ok(())
}

fn translate_sprite(
    render: &RenderSprite,
    stage: (u32, u32),
    texture_sizes: &BTreeMap<u32, (u32, u32)>,
) -> Result<Option<LegacyDrawV1>, LegacyProviderError> {
    let sprite = &render.sprite;
    if !sprite.visible {
        return Ok(None);
    }
    validate_basic_sprite(sprite)?;
    let image_id = sprite
        .image_id
        .ok_or_else(|| {
            invalid(
                "ASTRA_SIGLUS_SPRITE_TEXTURE",
                "visible sprite has no texture",
            )
        })?
        .0;
    let (texture_width, texture_height) =
        texture_sizes.get(&image_id).copied().ok_or_else(|| {
            invalid(
                "ASTRA_SIGLUS_SPRITE_TEXTURE",
                "texture metadata is unavailable",
            )
        })?;
    let (src_left, src_top, src_right, src_bottom) =
        source_rect(sprite.src_clip, texture_width, texture_height)?;
    let source_width = (src_right - src_left).max(1.0);
    let source_height = (src_bottom - src_top).max(1.0);
    let (x, y, width, height) = match sprite.fit {
        SpriteFit::FullScreen => (0.0, 0.0, stage.0 as f32, stage.1 as f32),
        SpriteFit::PixelRect => {
            let (width, height) = match sprite.size_mode {
                SpriteSizeMode::Intrinsic => (source_width, source_height),
                SpriteSizeMode::Explicit { width, height } => (width as f32, height as f32),
            };
            (sprite.x as f32, sprite.y as f32, width, height)
        }
    };
    let points = quad_points(sprite, x, y, width, height);
    let u0 = src_left / texture_width as f32;
    let v0 = src_top / texture_height as f32;
    let u1 = src_right / texture_width as f32;
    let v1 = src_bottom / texture_height as f32;
    let alpha = sprite.alpha as f32 / 255.0;
    let color = [1.0, 1.0, 1.0, alpha];
    Ok(Some(LegacyDrawV1 {
        texture_id: image_id,
        vertices: [
            vertex(points[3], [u0, v1], color),
            vertex(points[0], [u0, v0], color),
            vertex(points[2], [u1, v1], color),
            vertex(points[1], [u1, v0], color),
        ],
        blend: map_blend(sprite.blend)?,
        texture_filter: LegacyTextureFilter::Linear,
        scissor: sprite.dst_clip.map(map_scissor),
    }))
}

fn validate_basic_sprite(sprite: &Sprite) -> Result<(), LegacyProviderError> {
    if sprite.mesh_kind != 0
        || sprite.mask_image_id.is_some()
        || sprite.tonecurve_image_id.is_some()
        || sprite.fog_texture_image_id.is_some()
        || sprite.wipe_src_image_id.is_some()
        || sprite.wipe_fx_mode != 0
        || sprite.camera_enabled
        || sprite.billboard
        || sprite.light_enabled
        || sprite.fog_enabled
        || sprite.alpha_test
        || sprite.tr != 255
        || sprite.mono != 0
        || sprite.reverse != 0
        || sprite.bright != 0
        || sprite.dark != 0
        || sprite.color_rate != 0
        || sprite.color_add_r != 0
        || sprite.color_add_g != 0
        || sprite.color_add_b != 0
        || sprite.color_r != 0
        || sprite.color_g != 0
        || sprite.color_b != 0
        || sprite.rotate_x != 0.0
        || sprite.rotate_y != 0.0
        || sprite.z != 0.0
        || sprite.pivot_z != 0.0
        || sprite.scale_z != 1.0
    {
        return Err(invalid(
            "ASTRA_SIGLUS_SCENE_FEATURE_UNIMPLEMENTED",
            "hosted sprite requires typed scene features that are not implemented",
        ));
    }
    Ok(())
}

fn source_rect(
    clip: Option<ClipRect>,
    width: u32,
    height: u32,
) -> Result<(f32, f32, f32, f32), LegacyProviderError> {
    match clip {
        None => Ok((0.0, 0.0, width as f32, height as f32)),
        Some(clip)
            if clip.left >= 0
                && clip.top >= 0
                && clip.right > clip.left
                && clip.bottom > clip.top
                && clip.right as u32 <= width
                && clip.bottom as u32 <= height =>
        {
            Ok((
                clip.left as f32,
                clip.top as f32,
                clip.right as f32,
                clip.bottom as f32,
            ))
        }
        Some(_) => Err(invalid(
            "ASTRA_SIGLUS_SOURCE_CLIP",
            "source clip is invalid",
        )),
    }
}

fn quad_points(sprite: &Sprite, x: f32, y: f32, width: f32, height: f32) -> [[f32; 2]; 4] {
    let anchor_x = if sprite.object_anchor {
        x + sprite.texture_center_x
    } else {
        x
    };
    let anchor_y = if sprite.object_anchor {
        y + sprite.texture_center_y
    } else {
        y
    };
    let mut points = [[0.0, 0.0], [width, 0.0], [width, height], [0.0, height]];
    let (sin, cos) = sprite.rotate.sin_cos();
    for point in &mut points {
        let local_x = (point[0] - sprite.pivot_x) * sprite.scale_x;
        let local_y = (point[1] - sprite.pivot_y) * sprite.scale_y;
        point[0] = anchor_x + sprite.pivot_x + local_x * cos - local_y * sin;
        point[1] = anchor_y + sprite.pivot_y + local_x * sin + local_y * cos;
    }
    points
}

fn vertex(position: [f32; 2], tex_coord: [f32; 2], color: [f32; 4]) -> LegacyVertexV1 {
    LegacyVertexV1 {
        position,
        tex_coord,
        color,
    }
}

fn map_blend(blend: SpriteBlend) -> Result<LegacyBlendMode, LegacyProviderError> {
    match blend {
        SpriteBlend::Normal => Ok(LegacyBlendMode::Alpha),
        SpriteBlend::Add => Ok(LegacyBlendMode::Add),
        SpriteBlend::Mul => Ok(LegacyBlendMode::Multiply),
        SpriteBlend::Screen => Ok(LegacyBlendMode::Screen),
        SpriteBlend::Sub | SpriteBlend::Overlay => Err(invalid(
            "ASTRA_SIGLUS_BLEND_UNIMPLEMENTED",
            "Siglus blend mode has no exact shared renderer mapping",
        )),
    }
}

fn map_scissor(clip: ClipRect) -> LegacyScissorV1 {
    LegacyScissorV1 {
        x: clip.left,
        y: clip.top,
        width: clip.right.saturating_sub(clip.left),
        height: clip.bottom.saturating_sub(clip.top),
    }
}

fn require_session<'a>(
    sessions: &'a BTreeMap<String, SiglusSession>,
    session: &LegacyRuntimeSessionId,
) -> Result<&'a SiglusSession, LegacyProviderError> {
    sessions
        .get(&session.0)
        .ok_or_else(|| invalid("ASTRA_SIGLUS_SESSION_MISSING", "session is not active"))
}

fn invalid(code: &'static str, message: impl Into<String>) -> LegacyProviderError {
    LegacyProviderError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use astra_byte_source::{ByteRange, ByteSourceStat, RangeReadResult, SourceRevision};

    use super::*;

    struct MemoryVfs(BTreeMap<String, Vec<u8>>);

    impl LegacyVfsReader for MemoryVfs {
        fn stat_file(
            &self,
            _mount_set_id: &str,
            uri: &str,
        ) -> Result<ByteSourceStat, LegacyProviderError> {
            let bytes = self
                .0
                .get(uri)
                .ok_or_else(|| invalid("TEST_RESOURCE_MISSING", "fixture resource is missing"))?;
            Ok(ByteSourceStat {
                len: bytes.len() as u64,
                revision: SourceRevision(1),
            })
        }

        fn read_file_range(
            &self,
            _mount_set_id: &str,
            uri: &str,
            expected_revision: SourceRevision,
            range: ByteRange,
            max_bytes: u64,
        ) -> Result<RangeReadResult, LegacyProviderError> {
            if expected_revision != SourceRevision(1) || range.len > max_bytes {
                return Err(invalid("TEST_RANGE", "fixture range request is invalid"));
            }
            let bytes = self
                .0
                .get(uri)
                .ok_or_else(|| invalid("TEST_RESOURCE_MISSING", "fixture resource is missing"))?;
            let start = usize::try_from(range.offset)
                .map_err(|_| invalid("TEST_RANGE", "fixture range offset overflowed"))?;
            let len = usize::try_from(range.len)
                .map_err(|_| invalid("TEST_RANGE", "fixture range length overflowed"))?;
            let end = start
                .checked_add(len)
                .ok_or_else(|| invalid("TEST_RANGE", "fixture range overflowed"))?;
            let selected = bytes
                .get(start..end)
                .ok_or_else(|| invalid("TEST_RANGE", "fixture range is out of bounds"))?;
            Ok(RangeReadResult {
                range,
                revision: expected_revision,
                bytes: OwnedByteBuffer::from_vec(selected.to_vec()),
            })
        }

        fn enumerate_by_extension(
            &self,
            _mount_set_id: &str,
            root: &str,
            extension_without_dot: &str,
            max_entries: u32,
        ) -> Result<Vec<LegacyVfsListedFile>, LegacyProviderError> {
            let suffix = format!(".{extension_without_dot}");
            let mut entries = self
                .0
                .iter()
                .filter(|(uri, _)| uri.starts_with(root) && uri.ends_with(&suffix))
                .map(|(uri, bytes)| LegacyVfsListedFile {
                    uri: uri.clone(),
                    stat: ByteSourceStat {
                        len: bytes.len() as u64,
                        revision: SourceRevision(1),
                    },
                })
                .collect::<Vec<_>>();
            entries.sort_by(|left, right| left.uri.cmp(&right.uri));
            if entries.len() > max_entries as usize {
                return Err(invalid(
                    "TEST_ENUM",
                    "fixture enumeration exceeded its bound",
                ));
            }
            Ok(entries)
        }
    }

    fn empty_scene_chunk() -> Vec<u8> {
        let mut fields = [0i32; 33];
        fields[0] = (fields.len() * 4) as i32;
        fields[1] = (fields.len() * 4 + 4) as i32;
        for index in [3, 5, 7] {
            fields[index] = (fields.len() * 4 + 4) as i32;
        }
        fields[9] = (fields.len() * 4) as i32;
        fields[10] = 1;
        let mut bytes = fields
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>();
        bytes.extend_from_slice(&0i32.to_le_bytes());
        bytes
    }

    fn one_scene_pack(name: &str, scene: &[u8]) -> Vec<u8> {
        let name_utf16 = name.encode_utf16().collect::<Vec<_>>();
        let header_len = 23 * 4;
        let name_index_offset = header_len;
        let name_list_offset = name_index_offset + 8;
        let scene_index_offset = name_list_offset + name_utf16.len() * 2;
        let scene_data_offset = scene_index_offset + 8;
        let mut fields = [0i32; 23];
        fields[0] = header_len as i32;
        fields[13] = name_index_offset as i32;
        fields[14] = 1;
        fields[15] = name_list_offset as i32;
        fields[16] = 1;
        fields[17] = scene_index_offset as i32;
        fields[18] = 1;
        fields[19] = scene_data_offset as i32;
        fields[20] = 1;
        let mut bytes = fields
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>();
        bytes.extend_from_slice(&0i32.to_le_bytes());
        bytes.extend_from_slice(&(name_utf16.len() as i32).to_le_bytes());
        for value in name_utf16 {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0i32.to_le_bytes());
        bytes.extend_from_slice(&(scene.len() as i32).to_le_bytes());
        bytes.extend_from_slice(scene);
        bytes
    }

    fn context() -> LegacyRuntimeHostCtx {
        LegacyRuntimeHostCtx {
            case_id: "siglus-public".into(),
            package_id: "siglus-public".into(),
            package_hash: Hash256::from_sha256(b"siglus-public"),
            mount_set_id: "fixture".into(),
            media_service_ids: vec!["media".into()],
            permission_policy_id: "siglus-public".into(),
            report_sink_id: "siglus-public".into(),
            target: "headless".into(),
            profile: "e2".into(),
        }
    }

    #[test]
    fn public_fixture_runs_through_family_v8_lifecycle() {
        let gameexe =
            b"#START_SCENE = \"start\", 0\n#SCREEN_SIZE = 640, 480\n#WINDOW_TITLE = \"fixture\"\n"
                .to_vec();
        let vfs = Arc::new(MemoryVfs(
            [
                ("legacy://fixture/Gameexe.dat".into(), gameexe.clone()),
                (
                    "legacy://fixture/Scene.pck".into(),
                    one_scene_pack("start", &empty_scene_chunk()),
                ),
            ]
            .into_iter()
            .collect(),
        ));
        let mut provider = SiglusRuntimeProvider::new(vfs, None);
        let ctx = context();
        let report = provider
            .probe(
                &ctx,
                LegacyProbeRequest {
                    root_mount_id: "fixture".into(),
                    candidate_uris: vec![
                        "legacy://fixture/Gameexe.dat".into(),
                        "legacy://fixture/Scene.pck".into(),
                    ],
                    marker_hashes: Vec::new(),
                    max_entries: 2,
                    max_metadata_bytes: 1024 * 1024,
                },
            )
            .unwrap();
        assert_eq!(report.content_identity, Hash256::from_sha256(&gameexe));
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("siglus-session".into()),
                    case_fingerprint: report.content_identity,
                    script_uri: "legacy://fixture/Gameexe.dat".into(),
                    fixed_delta_ns: 16_666_666,
                    session_seed: 7,
                    compatibility_profile: "siglus.original".into(),
                    family_options: BTreeMap::new(),
                },
            )
            .unwrap();
        let output = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    tick_index: 1,
                    delta_ns: 16_666_666,
                    session_seed: 7,
                    mode: LegacyReplayMode::Live,
                    input_edges: Vec::new(),
                    await_results: Vec::new(),
                    provider_results: Vec::new(),
                    budget: LegacyStepBudget {
                        max_instructions: 100_000,
                        max_effects: 1024,
                        max_trace_entries: 1024,
                    },
                },
            )
            .unwrap();
        assert_eq!(output.state_revision, 1);
        assert_eq!(output.live.scenes.len(), 1);
        assert_eq!(output.live.scenes[0].width, 640);
        assert_eq!(output.live.scenes[0].height, 480);
        let shutdown = provider.shutdown(&ctx, &session).unwrap();
        assert_eq!(shutdown.final_state_revision, 1);
        assert!(!provider.has_active_sessions());
    }
}
