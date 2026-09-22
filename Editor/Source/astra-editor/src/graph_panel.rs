use super::*;
use astra_vn_editor::parse_astra_source;

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
            let card =
                div()
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
                                    Button::new(SharedString::from(format!(
                                        "graph-edge-{index}-{edge_index}"
                                    )))
                                    .label(format!("{} → {}", edge.label, edge.target))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.select_command(edge.source_id.clone(), window, cx)
                                    }))
                                },
                            )),
                    );
            surface = surface.child(card);
        }
        div()
            .id("story-graph")
            .size_full()
            .overflow_scroll()
            .child("Select a state or route; edit its source properties in Details.")
            .child(surface)
            .into_any_element()
    }
}
