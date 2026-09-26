use crate::family::{core, error, Lease};
use crate::*;
use astra_emu_family_api::*;
use astra_media_core::TextureFrame;
use std::{collections::BTreeMap, sync::Arc};
mod input;
const MAX_INSTRUCTIONS: u32 = 1_048_576;
const MAX_SCRIPT_RESIDENT: u64 = 256 * 1024 * 1024;
pub(crate) struct CmvsSession {
    archive: Arc<CmvsArchive>,
    vm: CmvsPs2aVmState,
    scripts: BTreeMap<u16, CmvsScript>,
    scene: CmvsScene,
    frame: TextureFrame,
    selected: Option<CmvsTextureSlot>,
    phase: u64,
    suspended: bool,
    input: input::InputState,
    failure: Option<FamilyError>,
    _lease: Lease,
}
impl CmvsSession {
    pub(crate) fn new(
        archive: Arc<CmvsArchive>,
        entry: &str,
        window: WindowState,
        lease: Lease,
    ) -> FamilyResult<Self> {
        let mut vm = CmvsPs2aVmState::new(0);
        let script = archive
            .resolve_script_uri(entry)
            .and_then(|uri| archive.load_script(&uri, 0, &mut vm))
            .map_err(core)?;
        let default_font = archive.native_default_font().map_err(core)?;
        let mut scene = CmvsScene::new().map_err(core)?;
        scene.set_default_font(default_font);
        let frame = scene.clear(window.width, window.height).map_err(core)?;
        tracing::info!(event = "astra.emu.cmvs.session.open");
        Ok(Self {
            archive,
            vm,
            scripts: BTreeMap::from([(0, script)]),
            scene,
            frame,
            selected: None,
            phase: 0,
            suspended: false,
            input: input::InputState::default(),
            failure: None,
            _lease: lease,
        })
    }
    pub(crate) fn frame_info(&self) -> FamilyResult<FrameInfo> {
        let info = FrameInfo {
            width: self.frame.width,
            height: self.frame.height,
            logical_width: self.frame.width,
            logical_height: self.frame.height,
            stride: self
                .frame
                .width
                .checked_mul(4)
                .ok_or_else(|| error("ASTRA_EMU_CMVS_FRAME", "frame stride overflow"))?,
            format: FrameFormat::Rgba8Srgb {
                alpha: FrameAlpha::Opaque,
            },
        };
        info.validate()?;
        Ok(info)
    }
    fn load(&mut self, frame: u16, name: String) -> FamilyResult<()> {
        let script = self
            .archive
            .load_called_script(&name, frame, &mut self.vm)
            .map_err(core)?;
        let resident = self
            .scripts
            .iter()
            .filter(|(slot, _)| **slot != frame)
            .map(|(_, script)| u64::from(script.decoded_size))
            .sum::<u64>()
            + u64::from(script.decoded_size);
        if resident > MAX_SCRIPT_RESIDENT {
            return Err(error(
                "ASTRA_EMU_CMVS_SCRIPT_BOUND",
                "resident script byte limit exceeded",
            ));
        }
        self.scripts.insert(frame, script);
        self.vm.current_frame = frame;
        Ok(())
    }
    fn action(&mut self, action: CmvsPs2aVmAction) -> FamilyResult<()> {
        match action {
            CmvsPs2aVmAction::NativeText(action) => {
                match action {
                    CmvsNativeTextAction::Create { slot }
                    | CmvsNativeTextAction::SetFont { slot } => {
                        self.scene
                            .bind_text_font(&self.vm.native_text[&slot].font)
                            .map_err(core)?;
                    }
                    CmvsNativeTextAction::DrawStatic { slot, source } => {
                        self.scene
                            .append_text(&mut self.vm, slot, &source)
                            .map_err(core)?;
                        tracing::debug!(
                            event = "cmvs.text.static",
                            slot,
                            glyph_count = self.vm.native_text[&slot].glyphs.len()
                        );
                    }
                    CmvsNativeTextAction::Destroy { .. } | CmvsNativeTextAction::Clear { .. } => {}
                }
                Ok(())
            }
            CmvsPs2aVmAction::CallScript { frame, name } => self.load(frame, name),
            CmvsPs2aVmAction::ReloadRootScript { name } => self.load(0, name),
            CmvsPs2aVmAction::EnsurePathDirectories { buffer_offset } => self
                .archive
                .ensure_path_directories(&self.vm, buffer_offset)
                .map_err(core),
            CmvsPs2aVmAction::LoadTextureParentResource {
                parent_slot,
                resource,
            } => {
                let uri = self
                    .archive
                    .resolve_native_resource(&self.vm, CmvsResourceSearchBank::Texture, &resource)
                    .map_err(core)?;
                self.scene
                    .bind(CmvsTextureSlot::Parent(parent_slot), &uri, &self.archive)
                    .map_err(core)
            }
            CmvsPs2aVmAction::LoadTextureResource {
                parent_slot,
                child_id,
                resource,
            } => {
                let uri = self
                    .archive
                    .resolve_native_resource(&self.vm, CmvsResourceSearchBank::Texture, &resource)
                    .map_err(core)?;
                self.scene
                    .bind(
                        CmvsTextureSlot::Child {
                            parent: parent_slot,
                            child: child_id,
                        },
                        &uri,
                        &self.archive,
                    )
                    .map_err(core)
            }
            CmvsPs2aVmAction::CommitTextureSurface {
                parent_slot,
                child_id,
            } => {
                self.selected = Some(child_id.map_or(
                    CmvsTextureSlot::Parent(parent_slot),
                    |child| CmvsTextureSlot::Child {
                        parent: parent_slot,
                        child,
                    },
                ));
                Ok(())
            }
            CmvsPs2aVmAction::PlayCrossfadeAudio { .. }
            | CmvsPs2aVmAction::FadeOutAudio { .. }
            | CmvsPs2aVmAction::StopAudio => Err(error(
                "ASTRA_EMU_CMVS_MEDIA_UNBOUND",
                "native crossfade audio is not connected",
            )),
            CmvsPs2aVmAction::StorageRequest(_) => Err(error(
                "ASTRA_EMU_CMVS_STORAGE_UNBOUND",
                "native storage request is not connected",
            )),
            CmvsPs2aVmAction::PlayChannelSound { .. } => Err(error(
                "ASTRA_EMU_CMVS_MEDIA_UNBOUND",
                "native media channel is not connected",
            )),
            _ => Err(error(
                "ASTRA_EMU_CMVS_ACTION_UNBOUND",
                "native presentation or system action is not connected",
            )),
        }
    }
    fn tick(&mut self) -> FamilyResult<()> {
        rebuild_frame_entry_queue(&mut self.vm).map_err(core)?;
        self.vm.dispatch_stopped = false;
        for _ in 0..MAX_INSTRUCTIONS {
            if self.vm.dispatch_stopped {
                if let Some(selected) = self.selected {
                    if let Some(frame) = self
                        .scene
                        .render(&self.vm, selected, &self.archive)
                        .map_err(core)?
                    {
                        self.frame = frame;
                    }
                } else if self.scene.has_text_renderer() {
                    self.frame = self
                        .scene
                        .render_text(&self.vm, self.frame.width, self.frame.height)
                        .map_err(core)?;
                }
                return Ok(());
            }
            let script = self.scripts.get(&self.vm.current_frame).ok_or_else(|| {
                error("ASTRA_EMU_CMVS_SCRIPT_FRAME", "script frame is not loaded")
            })?;
            let instruction =
                frame_ps2a_instruction(script, self.vm.program_counter).map_err(core)?;
            if let Some(action) = execute_cmvs390_frame(&mut self.vm, &instruction).map_err(core)? {
                self.action(action).map_err(|mut failure| {
                    if let CmvsPs2aInstructionFrame::Command { command_id, .. } = instruction {
                        failure.message =
                            format!("CMVS command {command_id}: {}", failure.message).into();
                    }
                    failure
                })?;
            }
        }
        Err(error(
            "ASTRA_EMU_CMVS_STEP_BOUND",
            "instruction limit exceeded",
        ))
    }
    fn advance_inner(
        &mut self,
        elapsed_ns: u64,
        events: &[FamilyEvent],
    ) -> FamilyResult<AdvanceResponse> {
        let mut changed = false;
        for event in events {
            self.input.event(&mut self.vm, event);
            match event {
                FamilyEvent::WindowCloseRequested => {
                    return Ok(AdvanceResponse {
                        status: FamilyStatus::Finished,
                        ..AdvanceResponse::running()
                    })
                }
                FamilyEvent::WindowSuspended { suspended } => {
                    changed |= self.suspended != *suspended;
                    self.suspended = *suspended;
                }
                FamilyEvent::PointerMove { x, y } => {
                    self.vm.pointer_x = *x as i32;
                    self.vm.pointer_y = *y as i32;
                }
                FamilyEvent::TextInput { .. } => {
                    return Err(error(
                        "ASTRA_EMU_CMVS_INPUT_UNBOUND",
                        "native input handling is not connected",
                    ))
                }
                _ => {}
            }
        }
        if self.suspended || changed {
            self.phase = 0;
            return Ok(AdvanceResponse {
                reset_clock: true,
                ..AdvanceResponse::running()
            });
        }
        self.phase = self
            .phase
            .checked_add(
                elapsed_ns
                    .checked_mul(60)
                    .ok_or_else(|| error("ASTRA_EMU_CMVS_TIME", "clock overflow"))?,
            )
            .ok_or_else(|| error("ASTRA_EMU_CMVS_TIME", "clock overflow"))?;
        if self.phase / 1_000_000_000 > 600 {
            return Err(error(
                "ASTRA_EMU_CMVS_TIME",
                "elapsed time exceeds tick budget",
            ));
        }
        while self.phase >= 1_000_000_000 {
            self.phase -= 1_000_000_000;
            self.tick()?;
        }
        Ok(AdvanceResponse::running())
    }
}
impl FamilySession for CmvsSession {
    fn advance(
        &mut self,
        elapsed_ns: u64,
        events: &[FamilyEvent],
    ) -> FamilyResult<AdvanceResponse> {
        if let Some(failure) = &self.failure {
            return Err(failure.clone());
        }
        let result = self.advance_inner(elapsed_ns, events);
        if let Err(failure) = &result {
            self.failure = Some(failure.clone());
        }
        result
    }
    fn visit_frame(&self, visitor: &mut dyn FrameVisitor) -> FamilyResult<()> {
        if let Some(failure) = &self.failure {
            return Err(failure.clone());
        }
        visitor.accept(FrameView::from_slice(
            self.frame.rgba8.as_slice(),
            self.frame_info()?,
        )?)
    }
    fn close(self: Box<Self>) -> FamilyResult<()> {
        tracing::info!(event = "astra.emu.cmvs.session.close");
        Ok(())
    }
}
