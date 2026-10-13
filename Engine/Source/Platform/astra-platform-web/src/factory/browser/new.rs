use super::*;

impl CanvasResource {
    pub(super) fn new(
        title: String,
        width: u32,
        height: u32,
        visible: bool,
    ) -> Result<Self, PlatformError> {
        let window = web_sys::window().ok_or_else(|| web_error("window.create"))?;
        let document = window
            .document()
            .ok_or_else(|| web_error("window.create"))?;
        let canvas = document
            .create_element("canvas")
            .map_err(|_| web_error("window.create"))?
            .dyn_into::<HtmlCanvasElement>()
            .map_err(|_| web_error("window.create"))?;
        let aria_root = document
            .create_element("div")
            .map_err(|_| web_error("accessibility.web.create"))?
            .dyn_into::<HtmlElement>()
            .map_err(|_| web_error("accessibility.web.create"))?;
        aria_root.set_id("astra-accessibility-root");
        aria_root
            .set_attribute("role", "application")
            .map_err(|_| web_error("accessibility.web.create"))?;
        let aria_style = aria_root.style();
        aria_style
            .set_property("position", "fixed")
            .map_err(|_| web_error("accessibility.web.create"))?;
        aria_style
            .set_property("inset", "0")
            .map_err(|_| web_error("accessibility.web.create"))?;
        aria_style
            .set_property("pointer-events", "none")
            .map_err(|_| web_error("accessibility.web.create"))?;
        canvas.set_width(width);
        canvas.set_height(height);
        canvas.set_tab_index(0);
        canvas.set_id("astra-player-canvas");
        canvas
            .set_attribute("aria-label", &title)
            .map_err(|_| web_error("window.create"))?;
        if !visible {
            canvas
                .style()
                .set_property("display", "none")
                .map_err(|_| web_error("window.create"))?;
        }
        document
            .body()
            .ok_or_else(|| web_error("window.create"))?
            .append_child(&canvas)
            .map_err(|_| web_error("window.create"))?;
        document
            .body()
            .ok_or_else(|| web_error("accessibility.web.create"))?
            .append_child(&aria_root)
            .map_err(|_| web_error("accessibility.web.create"))?;
        Ok(Self {
            canvas,
            aria_root,
            aria_listeners: Vec::new(),
            aria_programmatic_focus: Rc::new(Cell::new(false)),
            listeners: BTreeMap::new(),
            global_listeners: Vec::new(),
        })
    }

    pub(super) fn update_accessibility(
        &mut self,
        window: WindowHandle,
        snapshot: &UiSemanticSnapshot,
        emitter: astra_platform::PlatformEventEmitter,
    ) -> Result<(), PlatformError> {
        snapshot
            .validate()
            .map_err(|_| web_error("accessibility.web.update"))?;
        self.aria_listeners.clear();
        self.aria_root.set_text_content(None);
        let document = self
            .aria_root
            .owner_document()
            .ok_or_else(|| web_error("accessibility.web.update"))?;
        let mut elements = BTreeMap::<String, Element>::new();
        let mut focused = None;
        for node in &snapshot.nodes {
            let element = document
                .create_element("div")
                .map_err(|_| web_error("accessibility.web.update"))?;
            element
                .set_attribute("role", aria_role(node.role))
                .map_err(|_| web_error("accessibility.web.update"))?;
            element
                .set_attribute("data-astra-semantic-id", &node.id)
                .map_err(|_| web_error("accessibility.web.update"))?;
            if let Some(name) = node.name.as_deref() {
                element
                    .set_attribute("aria-label", name)
                    .map_err(|_| web_error("accessibility.web.update"))?;
            }
            if let Some(description) = node.description.as_deref() {
                element
                    .set_attribute("aria-description", description)
                    .map_err(|_| web_error("accessibility.web.update"))?;
            }
            if let Some(value) = node.value.as_deref() {
                element
                    .set_attribute("aria-valuetext", value)
                    .map_err(|_| web_error("accessibility.web.update"))?;
            }
            for (property, attribute) in [
                ("range.value", "aria-valuenow"),
                ("range.min", "aria-valuemin"),
                ("range.max", "aria-valuemax"),
            ] {
                if let Some(value) = node.properties.get(property) {
                    element
                        .set_attribute(attribute, value)
                        .map_err(|_| web_error("accessibility.web.update"))?;
                }
            }
            element
                .set_attribute("aria-disabled", if node.enabled { "false" } else { "true" })
                .map_err(|_| web_error("accessibility.web.update"))?;
            element
                .set_attribute("aria-hidden", if node.hidden { "true" } else { "false" })
                .map_err(|_| web_error("accessibility.web.update"))?;
            element
                .set_attribute(
                    "aria-selected",
                    if node.selected { "true" } else { "false" },
                )
                .map_err(|_| web_error("accessibility.web.update"))?;
            if let Some(checked) = node.checked {
                element
                    .set_attribute("aria-checked", if checked { "true" } else { "false" })
                    .map_err(|_| web_error("accessibility.web.update"))?;
            }
            let style = element
                .dyn_ref::<HtmlElement>()
                .ok_or_else(|| web_error("accessibility.web.update"))?
                .style();
            style
                .set_property("position", "absolute")
                .map_err(|_| web_error("accessibility.web.update"))?;
            style
                .set_property("left", &format!("{}px", node.bounds_points.min.x))
                .map_err(|_| web_error("accessibility.web.update"))?;
            style
                .set_property("top", &format!("{}px", node.bounds_points.min.y))
                .map_err(|_| web_error("accessibility.web.update"))?;
            style
                .set_property(
                    "width",
                    &format!("{}px", node.bounds_points.max.x - node.bounds_points.min.x),
                )
                .map_err(|_| web_error("accessibility.web.update"))?;
            style
                .set_property(
                    "height",
                    &format!("{}px", node.bounds_points.max.y - node.bounds_points.min.y),
                )
                .map_err(|_| web_error("accessibility.web.update"))?;
            style
                .set_property("opacity", "0")
                .map_err(|_| web_error("accessibility.web.update"))?;
            if !node.actions.is_empty() {
                element
                    .set_attribute("tabindex", "0")
                    .map_err(|_| web_error("accessibility.web.update"))?;
            }
            if node
                .actions
                .contains(&astra_ui_core::UiSemanticAction::Activate)
            {
                self.bind_aria_action(
                    &element,
                    window,
                    node.id.clone(),
                    astra_ui_core::UiSemanticAction::Activate,
                    emitter.clone(),
                )?;
            }
            if node
                .actions
                .contains(&astra_ui_core::UiSemanticAction::Focus)
            {
                self.bind_aria_action(
                    &element,
                    window,
                    node.id.clone(),
                    astra_ui_core::UiSemanticAction::Focus,
                    emitter.clone(),
                )?;
            }
            if node
                .actions
                .contains(&astra_ui_core::UiSemanticAction::SetValue)
            {
                self.bind_aria_action(
                    &element,
                    window,
                    node.id.clone(),
                    astra_ui_core::UiSemanticAction::SetValue,
                    emitter.clone(),
                )?;
            }
            if node.role == UiSemanticRole::Slider {
                self.bind_aria_slider_keys(&element, window, node.id.clone(), emitter.clone())?;
            }
            if node.focused {
                focused = Some(element.clone());
            }
            elements.insert(node.id.clone(), element);
        }
        for node in &snapshot.nodes {
            let element = elements
                .get(&node.id)
                .ok_or_else(|| web_error("accessibility.web.update"))?;
            if let Some(parent) = node.parent_id.as_ref() {
                elements
                    .get(parent)
                    .ok_or_else(|| web_error("accessibility.web.update"))?
                    .append_child(element)
                    .map_err(|_| web_error("accessibility.web.update"))?;
            } else {
                self.aria_root
                    .append_child(element)
                    .map_err(|_| web_error("accessibility.web.update"))?;
            }
        }
        if let Some(focused) = focused.and_then(|element| element.dyn_into::<HtmlElement>().ok()) {
            self.aria_programmatic_focus.set(true);
            let focus_result = focused.focus();
            self.aria_programmatic_focus.set(false);
            focus_result.map_err(|_| web_error("accessibility.web.focus"))?;
        }
        Ok(())
    }

    pub(super) fn bind_aria_action(
        &mut self,
        element: &Element,
        window: WindowHandle,
        semantic_id: String,
        action: astra_ui_core::UiSemanticAction,
        emitter: astra_platform::PlatformEventEmitter,
    ) -> Result<(), PlatformError> {
        let (event, action, read_value) = match action {
            astra_ui_core::UiSemanticAction::Activate => ("click", "invoke", false),
            astra_ui_core::UiSemanticAction::Focus => ("focus", "focus", false),
            astra_ui_core::UiSemanticAction::SetValue => ("change", "set_value", true),
            _ => return Err(web_error("accessibility.web.bind_action")),
        };
        let target: EventTarget = element.clone().into();
        let focus_guard = self.aria_programmatic_focus.clone();
        let callback = Closure::wrap(Box::new(move |event: Event| {
            if action == "focus" && focus_guard.get() {
                return;
            }
            let value = read_value
                .then(|| {
                    event.target().and_then(|target| {
                        Reflect::get(&target, &JsValue::from_str("value"))
                            .ok()
                            .and_then(|value| value.as_string())
                    })
                })
                .flatten();
            let _ = emitter.emit(PlatformEventKind::AccessibilityAction {
                window,
                semantic_id: semantic_id.clone(),
                action: action.to_string(),
                value,
            });
        }) as Box<dyn FnMut(Event)>);
        target
            .add_event_listener_with_callback(event, callback.as_ref().unchecked_ref())
            .map_err(|_| web_error("accessibility.web.bind"))?;
        self.aria_listeners.push(AriaListener {
            target,
            event,
            callback,
        });
        Ok(())
    }

    pub(super) fn bind_aria_slider_keys(
        &mut self,
        element: &Element,
        window: WindowHandle,
        semantic_id: String,
        emitter: astra_platform::PlatformEventEmitter,
    ) -> Result<(), PlatformError> {
        let target: EventTarget = element.clone().into();
        let callback = Closure::wrap(Box::new(move |event: Event| {
            let Ok(event) = event.dyn_into::<KeyboardEvent>() else {
                return;
            };
            let action = match event.key().as_str() {
                "ArrowRight" | "ArrowUp" => "increment",
                "ArrowLeft" | "ArrowDown" => "decrement",
                _ => return,
            };
            event.prevent_default();
            let _ = emitter.emit(PlatformEventKind::AccessibilityAction {
                window,
                semantic_id: semantic_id.clone(),
                action: action.to_string(),
                value: None,
            });
        }) as Box<dyn FnMut(Event)>);
        target
            .add_event_listener_with_callback("keydown", callback.as_ref().unchecked_ref())
            .map_err(|_| web_error("accessibility.web.bind"))?;
        self.aria_listeners.push(AriaListener {
            target,
            event: "keydown",
            callback,
        });
        Ok(())
    }
}
