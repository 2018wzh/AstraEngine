use super::*;
use astra_vn_editor::{parse_astra_source, EditBatch};
use gpui_component::{
    list::ListItem,
    resizable::{h_resizable, resizable_panel, ResizableState},
    tree::{tree, TreeItem, TreeState},
};

pub(super) struct WorkspacePanels {
    tree: Entity<TreeState>,
    tree_revision: Option<(String, u64)>,
    filter: Entity<InputState>,
    selected: Option<String>,
    selected_version: u64,
    last_cursor: Option<(String, u64, u32)>,
    fields: Vec<(String, Entity<InputState>)>,
    split: Entity<ResizableState>,
    layout_epoch: usize,
    sizes: [f32; 3],
    mode: ViewMode,
    _filter_subscription: Subscription,
}

#[derive(Clone, Copy, PartialEq)]
enum ViewMode {
    Source,
    Graph,
    Timeline,
}

impl WorkspacePanels {
    pub fn new(window: &mut Window, cx: &mut Context<Editor>) -> Self {
        let filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search source and assets"));
        let subscription = cx.subscribe(&filter, |_, _, _: &InputEvent, cx| cx.notify());
        Self {
            tree: cx.new(|cx| TreeState::new(cx)),
            tree_revision: None,
            filter,
            selected: None,
            selected_version: 0,
            last_cursor: None,
            fields: Vec::new(),
            split: cx.new(|_| ResizableState::default()),
            layout_epoch: 0,
            sizes: [250., 560., 280.],
            mode: ViewMode::Source,
            _filter_subscription: subscription,
        }
    }
}

impl Editor {
    pub fn load_layout(&mut self) {
        let path = self.project.layout_path();
        if !path.exists() {
            return;
        }
        let result = (|| -> anyhow::Result<[f32; 3]> {
            let sizes: [f32; 3] = serde_json::from_slice(&std::fs::read(path)?)?;
            anyhow::ensure!(
                sizes
                    .iter()
                    .all(|s| s.is_finite() && (100.0..=8000.0).contains(s)),
                "Invalid panel sizes"
            );
            Ok(sizes)
        })();
        match result {
            Ok(sizes) => self.panels.sizes = sizes,
            Err(error) => self.status = format!("Layout reset: {error}"),
        }
    }

    fn select_command(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.select_command_at(id, true, window, cx);
    }

    pub fn follow_source_cursor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.input.read(cx).focus_handle(cx).is_focused(window) {
            return;
        }
        let document = self
            .project
            .documents
            .document(&self.project.active)
            .unwrap();
        let line = self.input.read(cx).cursor_position().line;
        let cursor = (document.path.clone(), document.version, line);
        if self.panels.last_cursor.as_ref() == Some(&cursor) {
            return;
        }
        self.panels.last_cursor = Some(cursor);
        let parsed = parse_astra_source(&document.path, &document.text);
        let id = parsed
            .ast
            .commands()
            .find(|c| c.line() == line as usize + 1)
            .and_then(|c| c.source_id())
            .map(str::to_string);
        if let Some(id) = id {
            self.select_command_at(id, false, window, cx);
        }
    }

    fn select_command_at(
        &mut self,
        id: String,
        navigate: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let document = self
            .project
            .documents
            .document(&self.project.active)
            .unwrap();
        let parsed = parse_astra_source(&document.path, &document.text);
        let Some(command) = parsed.ast.commands().find(|c| c.source_id() == Some(&id)) else {
            return;
        };
        self.panels.fields = command
            .attributes()
            .map(|attribute| {
                let input = cx.new(|cx| InputState::new(window, cx));
                input.update(cx, |input, cx| {
                    input.set_value(attribute.value().to_string(), window, cx)
                });
                (attribute.key().to_string(), input)
            })
            .collect();
        self.panels.selected = Some(id);
        self.panels.selected_version = document.version;
        let position = gpui_component::input::Position::new(
            command.line().saturating_sub(1) as u32,
            command.column().saturating_sub(1) as u32,
        );
        if navigate {
            self.input.update(cx, |input, cx| {
                input.set_cursor_position(position, window, cx)
            });
        }
        cx.notify();
    }

    fn apply_details(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = (|| -> anyhow::Result<()> {
            let id = self
                .panels
                .selected
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Select a command first"))?;
            let document = self.project.documents.document(&self.project.active)?;
            anyhow::ensure!(
                document.version == self.panels.selected_version,
                "Source changed; select the command again before applying details"
            );
            let mut edits = Vec::new();
            for (key, input) in &self.panels.fields {
                let batch = self.project.documents.attribute_edit(
                    &self.project.active,
                    id,
                    key,
                    &input.read(cx).value(),
                )?;
                edits.extend(batch.documents.into_iter().flat_map(|d| d.edits));
            }
            self.project.documents.apply(EditBatch {
                documents: vec![astra_vn_editor::DocumentEdits {
                    path: self.project.active.clone(),
                    version: document.version,
                    edits,
                }],
            })?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.preview = None;
                self.sync(window, cx);
                if let Some(id) = self.panels.selected.clone() {
                    self.select_command(id, window, cx);
                }
            }
            Err(error) => {
                self.status = error.to_string();
                cx.notify();
            }
        }
    }

    pub fn workspace_panels(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let document = self
            .project
            .documents
            .document(&self.project.active)
            .unwrap();
        let revision = (document.path.clone(), document.version);
        if self.panels.tree_revision.as_ref() != Some(&revision) {
            let parsed = parse_astra_source(&document.path, &document.text);
            let commands = parsed
                .ast
                .commands()
                .filter_map(|c| {
                    c.source_id().map(|id| {
                        (
                            c.indent(),
                            TreeItem::new(id.to_string(), format!("{}  {}", c.keyword(), id))
                                .expanded(true),
                        )
                    })
                })
                .collect::<Vec<_>>();
            fn nested(
                commands: &[(usize, TreeItem)],
                cursor: &mut usize,
                indent: usize,
            ) -> Vec<TreeItem> {
                let mut nodes: Vec<TreeItem> = Vec::new();
                while let Some((depth, item)) = commands.get(*cursor) {
                    if *depth < indent {
                        break;
                    }
                    if *depth > indent {
                        let children = nested(commands, cursor, *depth);
                        if let Some(last) = nodes.last_mut() {
                            last.children.extend(children);
                        } else {
                            nodes.extend(children);
                        }
                    } else {
                        nodes.push(item.clone());
                        *cursor += 1;
                    }
                }
                nodes
            }
            let nodes = nested(&commands, &mut 0, 0);
            self.panels
                .tree
                .update(cx, |tree, cx| tree.set_items(nodes, cx));
            self.panels.tree_revision = Some(revision);
        }
        let filter = self.panels.filter.read(cx).value().to_lowercase();
        let source_rows = self
            .project
            .documents
            .documents()
            .filter(|d| d.path.to_lowercase().contains(&filter))
            .map(|d| {
                let path = d.path.clone();
                let label = format!(
                    "{}{}",
                    path,
                    if self.project.documents.is_dirty(&path).unwrap() {
                        " *"
                    } else {
                        ""
                    }
                );
                Button::new(SharedString::from(format!("source-{path}")))
                    .label(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.cancel_agent();
                        if let Err(error) = this.project.activate(&path) {
                            this.status = error.to_string();
                        }
                        this.panels.selected = None;
                        this.panels.fields.clear();
                        this.sync(window, cx);
                    }))
            })
            .collect::<Vec<_>>();
        let assets = self
            .project
            .content
            .iter()
            .filter(|p| p.to_lowercase().contains(&filter))
            .map(|p| div().text_sm().child(p.clone()))
            .collect::<Vec<_>>();
        let owner = cx.entity().downgrade();
        let selected = self.panels.selected.clone();
        let outliner = tree(&self.panels.tree, move |ix, entry, _, _, _| {
            let id = entry.item().id.to_string();
            let owner = owner.clone();
            ListItem::new(ix)
                .pl(px(12.0 * entry.depth() as f32))
                .selected(selected.as_deref() == Some(&id))
                .child(entry.item().label.clone())
                .on_click(move |_, window, cx| {
                    let _ =
                        owner.update(cx, |this, cx| this.select_command(id.clone(), window, cx));
                })
        });
        let browser = div()
            .flex()
            .flex_col()
            .size_full()
            .gap_2()
            .p_2()
            .child("Content Browser")
            .child(Input::new(&self.panels.filter))
            .child(
                div()
                    .id("content-list")
                    .max_h(px(180.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(source_rows)
                    .children(assets),
            )
            .child("Outliner")
            .child(div().flex_1().min_h_0().child(outliner));
        let modes = [
            ("Source", ViewMode::Source),
            ("Graph", ViewMode::Graph),
            ("Timeline", ViewMode::Timeline),
        ];
        let toolbar = div()
            .flex()
            .gap_2()
            .children(modes.into_iter().map(|(label, mode)| {
                Button::new(label)
                    .label(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.panels.mode = mode;
                        cx.notify();
                    }))
            }))
            .child(
                Button::new("reset-layout")
                    .label("Reset layout")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.panels.split = cx.new(|_| ResizableState::default());
                        this.panels.layout_epoch += 1;
                        this.panels.sizes = [250., 560., 280.];
                        let path = this.project.layout_path();
                        if path.exists() {
                            if let Err(error) = std::fs::remove_file(path) {
                                this.status = format!("Could not reset saved layout: {error}");
                            }
                        }
                        cx.notify();
                    })),
            );
        let center = match self.panels.mode {
            ViewMode::Source => div()
                .flex_1()
                .min_h_0()
                .child(Input::new(&self.input).h_full())
                .into_any_element(),
            ViewMode::Graph => self.command_cards(false, cx),
            ViewMode::Timeline => self.command_cards(true, cx),
        };
        let details = div()
            .id("details")
            .overflow_y_scroll()
            .size_full()
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .child("Details")
            .child(
                self.panels
                    .selected
                    .clone()
                    .unwrap_or_else(|| "Select a source command".into()),
            )
            .children(self.panels.fields.iter().map(|(key, input)| {
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(key.clone())
                    .child(Input::new(input))
            }))
            .child(
                Button::new("apply-details")
                    .label("Apply properties as one batch")
                    .disabled(self.panels.fields.is_empty())
                    .on_click(cx.listener(|this, _, window, cx| this.apply_details(window, cx))),
            );
        let _ = window;
        h_resizable(("editor-workspace", self.panels.layout_epoch))
            .with_state(&self.panels.split)
            .on_resize(cx.listener(|this, state: &Entity<ResizableState>, _, cx| {
                let sizes = state
                    .read(cx)
                    .sizes()
                    .iter()
                    .map(|size| f32::from(*size))
                    .collect::<Vec<_>>();
                let result = (|| -> anyhow::Result<()> {
                    let path = this.project.layout_path();
                    std::fs::create_dir_all(path.parent().unwrap())?;
                    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
                    use std::io::Write;
                    temporary.write_all(&serde_json::to_vec(&sizes)?)?;
                    temporary.persist(path)?;
                    Ok(())
                })();
                if let Err(error) = result {
                    this.status = format!("Could not save layout: {error}");
                    cx.notify();
                }
            }))
            .child(
                resizable_panel()
                    .size(px(self.panels.sizes[0]))
                    .child(browser),
            )
            .child(
                resizable_panel().size(px(self.panels.sizes[1])).child(
                    div()
                        .size_full()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(toolbar)
                        .child(center),
                ),
            )
            .child(
                resizable_panel()
                    .size(px(self.panels.sizes[2]))
                    .child(details),
            )
            .into_any_element()
    }

    fn command_cards(&self, timeline: bool, cx: &mut Context<Self>) -> AnyElement {
        let document = self
            .project
            .documents
            .document(&self.project.active)
            .unwrap();
        let parsed = parse_astra_source(&document.path, &document.text);
        let commands = parsed.ast.commands().filter(|c| {
            if timeline {
                matches!(
                    c.keyword(),
                    "timeline" | "move" | "transition" | "camera" | "shake" | "wait"
                )
            } else {
                matches!(
                    c.keyword(),
                    "story" | "state" | "scene" | "jump" | "branch" | "choice" | "option"
                )
            }
        });
        div()
            .id("command-cards")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .children(
                commands
                    .filter_map(|c| c.source_id().map(|id| (id.to_string(), c)))
                    .map(|(id, c)| {
                        let label = format!("{}  {}", c.keyword(), id);
                        let properties = c
                            .attributes()
                            .map(|a| format!("{}: {}", a.key(), a.value()))
                            .collect::<Vec<_>>()
                            .join("  ·  ");
                        div()
                            .border_1()
                            .border_color(rgb(0x3c4655))
                            .rounded_md()
                            .p_3()
                            .child(
                                Button::new(SharedString::from(id.clone()))
                                    .label(label)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.select_command(id.clone(), window, cx)
                                    })),
                            )
                            .child(properties)
                    }),
            )
            .into_any_element()
    }
}
