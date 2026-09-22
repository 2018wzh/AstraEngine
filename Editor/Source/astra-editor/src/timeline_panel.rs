use super::*;
use astra_editor::timeline::{self, Keyframe, KeyframeEdit};

#[derive(Clone)]
struct KeyframeDrag {
    session: uuid::Uuid,
    path: String,
    source_id: String,
    version: u64,
    index: usize,
    frame: Keyframe,
    duration: u32,
    start_x: std::rc::Rc<std::cell::Cell<f32>>,
}
impl Render for KeyframeDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .p_2()
            .bg(rgb(0x405878))
            .child(format!("Key at {} ms", self.frame.time_ms))
    }
}
pub(super) struct TimelineSelection {
    path: String,
    source_id: String,
    version: u64,
    index: usize,
    time: Entity<InputState>,
    value: Entity<InputState>,
}

impl TimelineSelection {
    pub(super) fn matches(&self, path: &str, id: &str, version: u64) -> bool {
        self.path == path && self.source_id == id && self.version == version
    }
}

impl Editor {
    fn select_keyframe(
        &mut self,
        source_id: String,
        version: u64,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let document = self
            .project
            .documents
            .document(&self.project.active)
            .unwrap();
        if document.version != version {
            self.status = "Timeline changed; select the keyframe again".into();
            cx.notify();
            return;
        }
        let frame = timeline::tracks(document)
            .ok()
            .and_then(|tracks| {
                tracks
                    .into_iter()
                    .find(|track| track.source_id == source_id)
            })
            .and_then(|track| track.frames.get(index).cloned());
        let Some(frame) = frame else {
            return;
        };
        let time = cx.new(|cx| InputState::new(window, cx).placeholder("Time in milliseconds"));
        let value = cx.new(|cx| InputState::new(window, cx).placeholder("Scalar value"));
        time.update(cx, |input, cx| {
            input.set_value(frame.time_ms.to_string(), window, cx)
        });
        value.update(cx, |input, cx| input.set_value(frame.value, window, cx));
        self.panels.timeline_selection = Some(TimelineSelection {
            path: document.path.clone(),
            source_id: source_id.clone(),
            version,
            index,
            time,
            value,
        });
        self.select_command(source_id, window, cx);
    }

    fn edit_keyframe(&mut self, action: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let result = (|| -> anyhow::Result<()> {
            let selected = self
                .panels
                .timeline_selection
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Select a keyframe"))?;
            let edit = if action == "remove" {
                KeyframeEdit::Remove {
                    index: selected.index,
                }
            } else {
                let frame = Keyframe {
                    time_ms: selected.time.read(cx).value().parse()?,
                    value: selected.value.read(cx).value().to_string(),
                };
                if action == "insert" {
                    KeyframeEdit::Insert(frame)
                } else {
                    KeyframeEdit::Set {
                        index: selected.index,
                        frame,
                    }
                }
            };
            let batch = timeline::edit(
                &self.project.documents,
                &selected.path,
                selected.version,
                &selected.source_id,
                edit,
            )?;
            self.project.documents.apply(batch)?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.cancel_agent();
                self.panels.timeline_selection = None;
                self.sync(window, cx);
            }
            Err(error) => {
                self.status = error.to_string();
                cx.notify();
            }
        }
    }

    fn drop_keyframe(&mut self, drag: &KeyframeDrag, window: &mut Window, cx: &mut Context<Self>) {
        let delta = f32::from(window.mouse_position().x) - drag.start_x.get();
        let time_ms = timeline::drag_time(drag.frame.time_ms, delta, drag.duration);
        if time_ms == drag.frame.time_ms {
            return;
        }
        let result = timeline::edit(
            &self.project.documents,
            &drag.path,
            drag.version,
            &drag.source_id,
            KeyframeEdit::Set {
                index: drag.index,
                frame: Keyframe {
                    time_ms,
                    value: drag.frame.value.clone(),
                },
            },
        )
        .and_then(|batch| Ok(self.project.documents.apply(batch)?));
        match result {
            Ok(_) => {
                self.cancel_agent();
                self.panels.timeline_selection = None;
                self.sync(window, cx);
            }
            Err(error) => {
                self.status = error.to_string();
                cx.notify();
            }
        }
    }
    pub(super) fn timeline_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let document = self
            .project
            .documents
            .document(&self.project.active)
            .unwrap();
        let tracks = match timeline::tracks(document) {
            Ok(tracks) => tracks,
            Err(error) => return div().p_3().child(error.to_string()).into_any_element(),
        };
        let version = document.version;
        let duration = tracks
            .iter()
            .filter_map(|track| track.frames.last().map(|frame| frame.time_ms))
            .max()
            .unwrap_or(1)
            .max(1);
        let mut panel = div()
            .id("timeline-panel")
            .size_full()
            .overflow_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .child("Timeline · drag keys to retime; select to edit · linear interpolation")
            .child(format!(
                "0 ms ────────────────────────────────────── {duration} ms"
            ));
        if tracks.is_empty() {
            panel = panel.child(
                "No timeline tracks in this source. Timeline commands are authored in .astra.",
            );
        }
        for track in tracks {
            let drop_id = track.source_id.clone();
            let drop_path = document.path.clone();
            let mut lane = div()
                .id(SharedString::from(format!("lane-{}", track.source_id)))
                .on_drop(cx.listener(move |this, drag: &KeyframeDrag, window, cx| {
                    if this.project.session_id() == drag.session
                        && drag.path == drop_path
                        && drag.source_id == drop_id
                        && this.project.active == drop_path
                    {
                        this.drop_keyframe(drag, window, cx);
                    }
                }))
                .relative()
                .w(px(640.))
                .h(px(44.))
                .bg(rgb(0x242c38))
                .border_b_1()
                .border_color(rgb(0x526078));
            for (index, frame) in track.frames.iter().enumerate() {
                let source_id = track.source_id.clone();
                let offset = frame.time_ms as f32 / duration as f32 * 520.;
                lane = lane.child(
                    div()
                        .id(SharedString::from(format!(
                            "drag-{}-{index}",
                            track.source_id
                        )))
                        .on_drag(
                            KeyframeDrag {
                                session: self.project.session_id(),
                                path: document.path.clone(),
                                source_id: track.source_id.clone(),
                                version,
                                index,
                                frame: frame.clone(),
                                duration,
                                start_x: std::rc::Rc::new(std::cell::Cell::new(0.)),
                            },
                            |drag, _, window, cx| {
                                drag.start_x.set(f32::from(window.mouse_position().x));
                                cx.new(|_| drag.clone())
                            },
                        )
                        .absolute()
                        .left(px(offset))
                        .top(px(5.))
                        .child(
                            Button::new(SharedString::from(format!(
                                "keyframe-{}-{index}",
                                track.source_id
                            )))
                            .label(format!("◆ {}", frame.time_ms))
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.select_keyframe(
                                        source_id.clone(),
                                        version,
                                        index,
                                        window,
                                        cx,
                                    )
                                },
                            )),
                        ),
                );
            }
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(format!(
                        "{} · {} · {}",
                        track.target, track.property, track.source_id
                    ))
                    .child(lane),
            );
        }
        if let Some(selected) = &self.panels.timeline_selection {
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(format!(
                        "{} · keyframe {} · v{}",
                        selected.source_id,
                        selected.index + 1,
                        selected.version
                    ))
                    .child("Time (ms)")
                    .child(Input::new(&selected.time))
                    .child("Value")
                    .child(Input::new(&selected.value))
                    .child(
                        div().flex().gap_2().children(
                            [
                                ("set", "Apply keyframe"),
                                ("insert", "Insert at time"),
                                ("remove", "Remove keyframe"),
                            ]
                            .into_iter()
                            .map(|(action, label)| {
                                Button::new(action).label(label).on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.edit_keyframe(action, window, cx)
                                    },
                                ))
                            }),
                        ),
                    ),
            );
        }
        panel.into_any_element()
    }
}
