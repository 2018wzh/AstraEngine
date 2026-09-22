use super::*;
use gpui_component::dock::{
    register_panel, DockArea, DockAreaState, DockEvent, DockItem, Panel, PanelEvent,
};

#[derive(Clone, Copy)]
enum PaneKind {
    Content,
    Authoring,
    Details,
}
impl PaneKind {
    fn name(self) -> &'static str {
        match self {
            Self::Content => "astra.content",
            Self::Authoring => "astra.authoring",
            Self::Details => "astra.details",
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Content => "Content & Outliner",
            Self::Authoring => "Authoring",
            Self::Details => "Details",
        }
    }
}
struct WorkspacePane {
    owner: WeakEntity<Editor>,
    kind: PaneKind,
    focus: FocusHandle,
    _subscription: Option<Subscription>,
}
impl WorkspacePane {
    fn new(owner: WeakEntity<Editor>, kind: PaneKind, cx: &mut Context<Self>) -> Self {
        let subscription = owner
            .upgrade()
            .map(|owner| cx.observe(&owner, |_, _, cx| cx.notify()));
        Self {
            owner,
            kind,
            focus: cx.focus_handle(),
            _subscription: subscription,
        }
    }
}
impl EventEmitter<PanelEvent> for WorkspacePane {}
impl Focusable for WorkspacePane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl Panel for WorkspacePane {
    fn panel_name(&self) -> &'static str {
        self.kind.name()
    }
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.kind.title()
    }
    fn closable(&self, _: &App) -> bool {
        false
    }
}
impl Render for WorkspacePane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self
            .owner
            .update(cx, |editor, cx| match self.kind {
                PaneKind::Content => editor.content_panel(window, cx),
                PaneKind::Authoring => editor.authoring_panel(cx),
                PaneKind::Details => editor.details_panel(cx),
            })
            .unwrap_or_else(|_| div().into_any_element());
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .child(content)
    }
}
impl Editor {
    pub(super) fn ensure_dock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panels.dock.is_some() {
            return;
        }
        let owner = cx.entity().downgrade();
        for kind in [PaneKind::Content, PaneKind::Authoring, PaneKind::Details] {
            let owner = owner.clone();
            register_panel(cx, kind.name(), move |_, _, _, _, cx| {
                Box::new(cx.new(|cx| WorkspacePane::new(owner.clone(), kind, cx)))
            });
        }
        let dock = cx.new(|cx| DockArea::new("astra-workspace", Some(1), window, cx));
        let weak_dock = dock.downgrade();
        let items = [PaneKind::Content, PaneKind::Authoring, PaneKind::Details]
            .into_iter()
            .zip([250., 560., 280.])
            .map(|(kind, width)| {
                let pane = cx.new(|cx| WorkspacePane::new(owner.clone(), kind, cx));
                DockItem::tab(pane, &weak_dock, window, cx).size(px(width))
            })
            .collect();
        let center = DockItem::h_split(items, &weak_dock, window, cx);
        dock.update(cx, |dock, cx| dock.set_center(center, window, cx));
        let path = self.project.layout_path();
        if path.exists() {
            let result = (|| -> anyhow::Result<()> {
                anyhow::ensure!(
                    std::fs::metadata(&path)?.len() <= 65536,
                    "Layout exceeds size limit"
                );
                let state: DockAreaState = serde_json::from_slice(&std::fs::read(&path)?)?;
                anyhow::ensure!(state.version == Some(1), "Layout version changed");
                validate_layout(&serde_json::to_value(&state)?)?;
                dock.update(cx, |dock, cx| dock.load(state, window, cx))?;
                Ok(())
            })();
            if let Err(error) = result {
                self.status = format!("Layout reset: {error}");
            }
        }
        self.panels.dock_subscription =
            Some(cx.subscribe(&dock, |this, dock, event: &DockEvent, cx| {
                if !matches!(event, DockEvent::LayoutChanged) {
                    return;
                }
                let result = (|| -> anyhow::Result<()> {
                    let state = dock.read(cx).dump(cx);
                    let path = this.project.layout_path();
                    std::fs::create_dir_all(path.parent().unwrap())?;
                    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
                    use std::io::Write;
                    temporary.write_all(&serde_json::to_vec(&state)?)?;
                    temporary.persist(path)?;
                    Ok(())
                })();
                if let Err(error) = result {
                    this.status = format!("Could not save layout: {error}");
                    cx.notify();
                }
            }));
        self.panels.dock = Some(dock);
    }
}

// Reject missing/duplicate panes before the component builds any views.
fn validate_layout(value: &serde_json::Value) -> anyhow::Result<()> {
    fn visit(value: &serde_json::Value, counts: &mut [usize; 3]) -> anyhow::Result<()> {
        match value {
            serde_json::Value::Object(fields) => {
                if let Some(name) = fields.get("panel_name").and_then(|v| v.as_str()) {
                    match name {
                        "astra.content" => counts[0] += 1,
                        "astra.authoring" => counts[1] += 1,
                        "astra.details" => counts[2] += 1,
                        "StackPanel" | "TabPanel" => (),
                        _ => anyhow::bail!("Unknown workspace pane"),
                    }
                }
                for value in fields.values() {
                    visit(value, counts)?;
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    visit(value, counts)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
    let mut counts = [0; 3];
    visit(value, &mut counts)?;
    anyhow::ensure!(
        counts == [1; 3],
        "Layout must contain each workspace pane exactly once"
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn saved_layout_cannot_drop_or_duplicate_authoring_panes() {
        let mut state = serde_json::json!({"panel_name":"StackPanel", "children":[
            {"panel_name":"astra.content"}, {"panel_name":"astra.authoring"}, {"panel_name":"astra.details"}]});
        validate_layout(&state).unwrap();
        state["children"][2]["panel_name"] = "astra.content".into();
        assert!(validate_layout(&state).is_err());
        state["children"][2]["panel_name"] = "unknown".into();
        assert!(validate_layout(&state).is_err());
    }
}
