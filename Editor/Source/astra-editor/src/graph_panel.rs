use super::*;
use astra_vn_editor::parse_astra_source;

#[derive(Clone)]
struct StateDrag {
    session: uuid::Uuid,
    path: String,
    version: u64,
    source_id: String,
}
impl Render for StateDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().p_2().bg(rgb(0x405878)).child("Connect state")
    }
}
struct Node {
    name: String,
    source_id: String,
    edges: Vec<Edge>,
}

struct Edge {
    source_id: String,
    label: String,
    target: String,
}

impl Editor {
    fn finish_graph_edit(
        &mut self,
        batch: anyhow::Result<astra_vn_editor::EditBatch>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match batch.and_then(|batch| Ok(self.project.documents.apply(batch)?)) {
            Ok(_) => {
                self.cancel_agent();
                self.sync(window, cx);
            }
            Err(error) => {
                self.status = error.to_string();
                cx.notify();
            }
        }
    }
    /// Rebuild the graph from source IDs and route targets; layout has no story authority.
    pub(super) fn graph_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let document = self
            .project
            .documents
            .document(&self.project.active)
            .unwrap();
        let parsed = parse_astra_source(&document.path, &document.text);
        let mut nodes = Vec::<Node>::new();
        for command in parsed.ast.commands() {
            let Some(source_id) = command.source_id() else {
                continue;
            };
            if command.keyword() == "state" {
                nodes.push(Node {
                    name: command
                        .arguments()
                        .next()
                        .map(|a| a.0.to_string())
                        .unwrap_or_default(),
                    source_id: source_id.into(),
                    edges: Vec::new(),
                });
            } else if matches!(command.keyword(), "jump" | "option" | "branch" | "call") {
                let Some(node) = nodes.last_mut() else {
                    continue;
                };
                let keys: &[&str] = if command.keyword() == "branch" {
                    &["then", "else"]
                } else {
                    &["target"]
                };
                for key in keys {
                    let target = command
                        .attribute(key)
                        .map(|a| a.value())
                        .or_else(|| command.arguments().next().map(|a| a.0));
                    if let Some(target) = target {
                        node.edges.push(Edge {
                            source_id: source_id.into(),
                            label: format!("{} {key}", command.keyword()),
                            target: target.into(),
                        });
                    }
                }
            }
        }
        if nodes.is_empty() {
            return div()
                .p_3()
                .child("No story states in this source.")
                .into_any_element();
        }
        let origin = |index: usize| {
            (
                24. + (index % 3) as f32 * 300.,
                24. + (index / 3) as f32 * 260.,
            )
        };
        let edges = nodes
            .iter()
            .enumerate()
            .flat_map(|(index, node)| {
                let nodes = &nodes;
                node.edges.iter().filter_map(move |edge| {
                    nodes
                        .iter()
                        .position(|target| target.name == edge.target)
                        .map(|target| (origin(index), origin(target)))
                })
            })
            .collect::<Vec<_>>();
        let height = nodes.len().div_ceil(3) as f32 * 260. + 30.;
        let mut surface = div().relative().w(px(930.)).h(px(height)).child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    for ((from_x, from_y), (to_x, to_y)) in &edges {
                        let from = bounds.origin + point(px(from_x + 250.), px(from_y + 95.));
                        let to = bounds.origin + point(px(*to_x), px(to_y + 95.));
                        let mut path = PathBuilder::stroke(px(1.5));
                        path.move_to(from);
                        path.line_to(point(from.x + px(20.), from.y));
                        path.line_to(point(to.x - px(12.), to.y));
                        path.line_to(to);
                        path.move_to(to + point(px(-6.), px(-4.)));
                        path.line_to(to);
                        path.line_to(to + point(px(-6.), px(4.)));
                        if let Ok(path) = path.build() {
                            window.paint_path(path, rgb(0x7b95bf));
                        }
                    }
                },
            )
            .absolute()
            .size_full(),
        );
        for (index, node) in nodes.into_iter().enumerate() {
            let (x, y) = origin(index);
            let id = node.source_id.clone();
            let path = document.path.clone();
            let version = document.version;
            let target = node.name.clone();
            let drag = StateDrag {
                session: self.project.session_id(),
                path: path.clone(),
                version,
                source_id: id.clone(),
            };
            let card =
                div()
                    .id(SharedString::from(format!("graph-card-{index}")))
                    .on_drop(cx.listener(move |this, drag: &StateDrag, window, cx| {
                        if this.project.session_id() != drag.session
                            || this.project.active != drag.path
                            || drag.path != path
                        {
                            return;
                        }
                        let batch = astra_editor::graph::connect(
                            &this.project.documents,
                            &drag.path,
                            drag.version,
                            &drag.source_id,
                            &target,
                        );
                        this.finish_graph_edit(batch, window, cx);
                    }))
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(250.))
                    .h(px(210.))
                    .p_2()
                    .border_1()
                    .border_color(rgb(0x60728d))
                    .rounded_md()
                    .bg(rgb(0x242c38))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .id(SharedString::from(format!("graph-port-{index}")))
                            .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
                            .child("Drag to connect →"),
                    )
                    .child(
                        Button::new(SharedString::from(format!("graph-node-{id}")))
                            .label(node.name)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_command(id.clone(), window, cx)
                            })),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("graph-edges-{index}")))
                            .flex_1()
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .children(node.edges.into_iter().enumerate().map(
                                |(edge_index, edge)| {
                                    let remove_id = edge.source_id.clone();
                                    let remove_path = document.path.clone();
                                    div()
                                        .flex()
                                        .gap_1()
                                        .child(
                                            Button::new(SharedString::from(format!(
                                                "graph-edge-{index}-{edge_index}"
                                            )))
                                            .label(format!("{} → {}", edge.label, edge.target))
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.select_command(
                                                    edge.source_id.clone(),
                                                    window,
                                                    cx,
                                                )
                                            })),
                                        )
                                        .child(
                                            Button::new(SharedString::from(format!(
                                                "remove-route-{index}-{edge_index}"
                                            )))
                                            .label("Delete route")
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                let batch = astra_editor::graph::remove(
                                                    &this.project.documents,
                                                    &remove_path,
                                                    version,
                                                    &remove_id,
                                                );
                                                this.finish_graph_edit(batch, window, cx);
                                            })),
                                        )
                                },
                            )),
                    );
            surface = surface.child(card);
        }
        div()
            .id("story-graph")
            .size_full()
            .overflow_scroll()
            .child("Drag a state port onto a target to add a jump. Delete route removes its command (both branch ports).")
            .child(surface)
            .into_any_element()
    }
}
