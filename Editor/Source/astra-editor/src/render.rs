use super::*;

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let document = self
            .project
            .documents
            .document(&self.project.active)
            .unwrap();
        let dirty = self
            .project
            .documents
            .is_dirty(&self.project.active)
            .unwrap();
        div().flex().flex_col().size_full().p_4().gap_3().bg(rgb(0x181b22)).text_color(rgb(0xe4e8ef))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if (event.keystroke.modifiers.control || event.keystroke.modifiers.platform) && event.keystroke.key == "s" {
                    this.status = this.project.save_all().map(|_| "All sources saved".into()).unwrap_or_else(|e| e.to_string());
                    cx.stop_propagation(); cx.notify();
                }
            }))
            .capture_action(cx.listener(|this, _: &gpui_component::input::Undo, window, cx| {
                if this.input.read(cx).focus_handle(cx).is_focused(window) {
                    if let Err(error) = this.project.documents.undo() { this.status = error.to_string(); } else { this.preview = None; this.sync(window, cx); }
                    cx.stop_propagation(); cx.notify();
                }
            }))
            .capture_action(cx.listener(|this, _: &gpui_component::input::Redo, window, cx| {
                if this.input.read(cx).focus_handle(cx).is_focused(window) {
                    if let Err(error) = this.project.documents.redo() { this.status = error.to_string(); } else { this.preview = None; this.sync(window, cx); }
                    cx.stop_propagation(); cx.notify();
                }
            }))
            .child(div().flex().gap_3().items_center()
                .child(format!("{}{} · v{}", self.project.active, if dirty { " *" } else { "" }, document.version))
                .child(Button::new("save").label("Save").on_click(cx.listener(|this, _, _, cx| {
                    this.status = this.project.save_all().map(|_| "All sources saved".to_string()).unwrap_or_else(|e| e.to_string()); cx.notify();
                })))
                .child(Button::new("undo").label("Undo batch").on_click(cx.listener(|this, _, window, cx| {
                    match this.project.documents.undo() { Ok(()) => this.sync(window, cx), Err(e) => { this.status = e.to_string(); cx.notify(); } }
                })))
                .child(Button::new("redo").label("Redo").on_click(cx.listener(|this, _, window, cx| {
                    match this.project.documents.redo() { Ok(()) => this.sync(window, cx), Err(e) => { this.status = e.to_string(); cx.notify(); } }
                })))
                .child(Button::new("preview").label("Save & Preview").on_click(cx.listener(|this, _, _, cx| {
                    let result = (|| -> anyhow::Result<()> {
                        let config = this.preview_config.as_ref().ok_or_else(|| anyhow::anyhow!("Open with a preview configuration to bind project, target, profile and built tools"))?;
                        let compiled = this.project.compile()?;
                        this.project.save_all()?;
                        this.preview = None;
                        this.preview_generation = this.preview_generation.checked_add(1).ok_or_else(|| anyhow::anyhow!("Preview generation exhausted"))?;
                        let identity = astra_vn_editor::PreviewIdentity {
                            project_hash: compiled.project_hash,
                            generation: this.preview_generation,
                            documents: this.project.documents.documents().map(|document| (document.path.clone(), astra_vn_editor::PreviewDocumentRevision {
                                version: document.version,
                                content_hash: astra_core::Hash256::from_sha256(document.text.as_bytes()),
                            })).collect(),
                        };
                        this.preview = Some(Preview::start(config, identity)?);
                        Ok(())
                    })();
                    if let Err(error) = result { this.status = error.to_string(); }
                    cx.notify();
                })))
                .child(Button::new("stop").label("Stop preview").on_click(cx.listener(|this, _, _, cx| {
                    if let Some(preview) = &mut this.preview { if let Err(error) = preview.stop() { this.status = error.to_string(); this.preview = None; } } cx.notify();
                }))))
            .child(self.preview_controls(cx))
            .child(div().flex_1().min_h_0().child(self.workspace_panels(window, cx)))
            .child(div().flex().gap_3().child(self.status.clone()).child(Button::new("locate-diagnostic").label("Go to diagnostic").disabled(self.diagnostic_position.is_none()).on_click(cx.listener(|this, _, window, cx| {
                if let (Some(position), Some(source)) = (this.diagnostic_position, this.diagnostic_source.clone()) {
                    match this.project.activate(&source) {
                        Ok(()) => { this.cancel_agent(); this.sync(window, cx); this.input.update(cx, |input, cx| input.set_cursor_position(position, window, cx)); }
                        Err(error) => { this.status = error.to_string(); cx.notify(); }
                    }
                }
            }))))
            .child(div().flex().gap_3()
                .child(Button::new("agent-mode").label(format!("Agent: {:?}", self.mode)).on_click(cx.listener(|this, _, _, cx| {
                    this.mode = if this.mode == EditMode::Autonomous { EditMode::ReviewEachBatch } else { EditMode::Autonomous };
                    this.cancel_agent(); cx.notify();
                })))
                .child(Button::new("cancel-agent").label("Cancel agent").on_click(cx.listener(|this, _, _, cx| { this.cancel_agent(); cx.notify(); })))
                .child(Button::new("approve").label("Apply batch").disabled(self.pending_reply.is_none()).on_click(cx.listener(|this, _, window, cx| { this.review(true, window, cx); })))
                .child(Button::new("reject").label("Reject batch").disabled(self.pending_reply.is_none()).on_click(cx.listener(|this, _, window, cx| { this.review(false, window, cx); }))))
            .child(div().id("pending-patch").max_h(px(180.)).overflow_y_scroll().child(self.agent.pending().map(|batch| serde_json::to_string_pretty(batch).unwrap_or_default()).unwrap_or_default()))
            .child(div().flex().gap_3().child(Input::new(&self.agent_prompt)).child(Button::new("send-agent").label("Send").on_click(cx.listener(|this, _, _, cx| { this.prompt_agent(cx); cx.notify(); }))))
            .child(div().id("agent-output").max_h(px(120.)).overflow_y_scroll().child(self.agent_output.clone()))
            .child(self.preview.as_ref().map(|p| p.status.clone()).unwrap_or_default())
    }
}

impl Editor {
    fn preview_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut controls = div().flex().gap_2();
        if let Some(live) = self
            .preview
            .as_ref()
            .and_then(|preview| preview.live.as_ref())
        {
            let paused = live.paused;
            controls = controls.child(
                Button::new("pause-preview")
                    .label(if paused { "Resume" } else { "Pause" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(preview) = &mut this.preview {
                            if let Err(error) = preview.send(if paused {
                                astra_vn_editor::PreviewCommand::Resume
                            } else {
                                astra_vn_editor::PreviewCommand::Pause
                            }) {
                                this.status = error.to_string();
                            }
                        }
                        cx.notify();
                    })),
            );
            if paused {
                if let Some(source_id) = &live.source_id {
                    for checkpoint in &live.checkpoints {
                        let source_id = source_id.clone();
                        let id = checkpoint.id;
                        controls = controls.child(
                            Button::new(("seek-preview", id))
                                .label(format!(
                                    "{:.2}s",
                                    checkpoint.presentation_time_ns as f64 / 1_000_000_000.
                                ))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(preview) = &mut this.preview {
                                        if let Err(error) = preview.send(
                                            astra_vn_editor::PreviewCommand::SeekWithinFragment {
                                                source_id: source_id.clone(),
                                                checkpoint: id,
                                            },
                                        ) {
                                            this.status = error.to_string();
                                        }
                                    }
                                    cx.notify();
                                })),
                        );
                    }
                }
            }
        }
        div()
            .id("preview-controls")
            .overflow_x_scroll()
            .child(controls)
    }
}
