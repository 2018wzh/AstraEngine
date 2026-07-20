use yakui_core::context;
use yakui_core::event::{EventInterest, EventResponse, WidgetEvent};
use yakui_core::geometry::{Color, Constraints, Vec2};
use yakui_core::input::{KeyCode, MouseButton};
use yakui_core::paint::PaintRect;
use yakui_core::widget::{EventContext, LayoutContext, PaintContext, Widget};
use yakui_core::{Response, WidgetId};

#[derive(Debug, Clone)]
pub struct AstraNodeProps {
    pub semantic_id: String,
    pub min_size: Vec2,
    pub max_size: Vec2,
    pub fill: Color,
    pub interactive: bool,
    pub fill_width: bool,
    pub fill_height: bool,
    pub loose_children: bool,
    pub clip_children: bool,
}

#[derive(Debug, Clone)]
pub struct AstraNodeResponse {
    pub semantic_id: String,
    pub widget_id: WidgetId,
    pub clicked_semantic_id: Option<String>,
    pub focused: bool,
}

#[derive(Debug)]
pub struct AstraNodeWidget {
    props: AstraNodeProps,
    pressed: bool,
    clicked_semantic_id: Option<String>,
    focused: bool,
}

impl AstraNodeWidget {
    pub fn show<F: FnOnce()>(props: AstraNodeProps, children: F) -> Response<AstraNodeResponse> {
        let dom = context::dom();
        let response = dom.begin_widget::<Self>(props);
        children();
        dom.end_widget::<Self>(response.id);
        response
    }
}

impl Widget for AstraNodeWidget {
    type Props<'a> = AstraNodeProps;
    type Response = AstraNodeResponse;

    fn new() -> Self {
        Self {
            props: AstraNodeProps {
                semantic_id: "ui.uninitialized".into(),
                min_size: Vec2::ZERO,
                max_size: Vec2::INFINITY,
                fill: Color::CLEAR,
                interactive: false,
                fill_width: false,
                fill_height: false,
                loose_children: false,
                clip_children: false,
            },
            pressed: false,
            clicked_semantic_id: None,
            focused: false,
        }
    }

    fn update(&mut self, props: Self::Props<'_>) -> Self::Response {
        if self.props.semantic_id != props.semantic_id {
            self.pressed = false;
            self.clicked_semantic_id = None;
        }
        self.props = props;
        AstraNodeResponse {
            semantic_id: self.props.semantic_id.clone(),
            widget_id: context::dom().current(),
            clicked_semantic_id: self.clicked_semantic_id.take(),
            focused: self.focused,
        }
    }

    fn layout(&self, mut ctx: LayoutContext<'_>, constraints: Constraints) -> Vec2 {
        if self.props.clip_children {
            ctx.layout.enable_clipping(ctx.dom);
        }
        let node = ctx.dom.get_current();
        let mut size = self.props.min_size;
        let bounded_max = constraints.max.min(self.props.max_size);
        let child_constraints = if self.props.loose_children {
            Constraints::loose(bounded_max)
        } else {
            Constraints {
                min: constraints.min.min(bounded_max),
                max: bounded_max,
            }
        };
        for child in &node.children {
            size = size.max(ctx.calculate_layout(*child, child_constraints));
        }
        if self.props.fill_width && constraints.max.x.is_finite() {
            size.x = constraints.max.x;
        }
        if self.props.fill_height && constraints.max.y.is_finite() {
            size.y = constraints.max.y;
        }
        constraints
            .constrain(size)
            .min(self.props.max_size)
            .max(self.props.min_size)
    }

    fn paint(&self, mut ctx: PaintContext<'_>) {
        let node = ctx.dom.get_current();
        if self.props.fill.a != 0 {
            let layout = ctx
                .layout
                .get(ctx.dom.current())
                .expect("layout exists for a painted Astra node");
            let mut rect = PaintRect::new(layout.rect);
            rect.color = if self.pressed {
                self.props.fill.adjust(0.8)
            } else if self.focused {
                self.props.fill.adjust(1.18)
            } else {
                self.props.fill
            };
            rect.add(ctx.paint);
        }
        for child in &node.children {
            ctx.paint(*child);
        }
    }

    fn event_interest(&self) -> EventInterest {
        if self.props.interactive {
            EventInterest::MOUSE_INSIDE
                | EventInterest::MOUSE_OUTSIDE
                | EventInterest::FOCUS
                | EventInterest::FOCUSED_KEYBOARD
        } else {
            EventInterest::empty()
        }
    }

    fn event(&mut self, ctx: EventContext<'_>, event: &WidgetEvent) -> EventResponse {
        if !self.props.interactive {
            return EventResponse::Bubble;
        }
        match event {
            WidgetEvent::FocusChanged(focused) => {
                self.focused = *focused;
                EventResponse::Sink
            }
            WidgetEvent::MouseButtonChanged {
                button: MouseButton::One,
                down,
                inside,
                ..
            } => {
                if *down && *inside {
                    self.pressed = true;
                    ctx.input.set_selection(Some(ctx.dom.current()));
                    EventResponse::Sink
                } else if !down && self.pressed {
                    self.pressed = false;
                    if *inside {
                        self.clicked_semantic_id = Some(self.props.semantic_id.clone());
                    }
                    EventResponse::Sink
                } else {
                    EventResponse::Bubble
                }
            }
            WidgetEvent::KeyChanged {
                key: KeyCode::Enter | KeyCode::Space,
                down: true,
                ..
            } if self.focused => {
                self.clicked_semantic_id = Some(self.props.semantic_id.clone());
                EventResponse::Sink
            }
            _ => EventResponse::Bubble,
        }
    }
}
