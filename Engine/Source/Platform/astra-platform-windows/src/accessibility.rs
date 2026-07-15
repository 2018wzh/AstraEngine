#![cfg(target_os = "windows")]

use std::{
    collections::BTreeMap,
    sync::{mpsc, Arc, Mutex},
};

use accesskit::{
    Action, ActionData, ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, Node,
    NodeId, Rect, Role, Toggled, Tree, TreeId, TreeUpdate,
};
use accesskit_winit::Adapter;
use astra_platform::{PlatformError, PlatformErrorCode, WindowHandle};
use astra_ui_core::{UiSemanticAction, UiSemanticRole, UiSemanticSnapshot, ValidateUi};
use winit::{event::WindowEvent, event_loop::ActiveEventLoop, window::Window};

#[derive(Debug)]
pub(crate) struct AccessibilityActionRequest {
    pub window: WindowHandle,
    pub semantic_id: String,
    pub action: String,
    pub value: Option<String>,
}

struct InitialTreeHandler {
    latest: Arc<Mutex<Option<TreeUpdate>>>,
}

impl ActivationHandler for InitialTreeHandler {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.latest.lock().ok()?.clone()
    }
}

struct RequestHandler {
    window: WindowHandle,
    ids: Arc<Mutex<BTreeMap<NodeId, String>>>,
    tx: mpsc::Sender<AccessibilityActionRequest>,
}

impl ActionHandler for RequestHandler {
    fn do_action(&mut self, request: ActionRequest) {
        if request.target_tree != TreeId::ROOT {
            return;
        }
        let Some(semantic_id) = self
            .ids
            .lock()
            .ok()
            .and_then(|ids| ids.get(&request.target_node).cloned())
        else {
            return;
        };
        let Some(action) = action_name(request.action) else {
            return;
        };
        let value = match request.data {
            Some(ActionData::Value(value)) => Some(value.into_string()),
            Some(ActionData::NumericValue(value)) => Some(value.to_string()),
            _ => None,
        };
        let _ = self.tx.send(AccessibilityActionRequest {
            window: self.window,
            semantic_id,
            action: action.to_string(),
            value,
        });
    }
}

struct DeactivateHandler;

impl DeactivationHandler for DeactivateHandler {
    fn deactivate_accessibility(&mut self) {}
}

pub(crate) struct WindowsAccessibilityBridge {
    adapter: Adapter,
    latest: Arc<Mutex<Option<TreeUpdate>>>,
    ids: Arc<Mutex<BTreeMap<NodeId, String>>>,
    action_rx: mpsc::Receiver<AccessibilityActionRequest>,
}

impl WindowsAccessibilityBridge {
    pub(crate) fn new(event_loop: &ActiveEventLoop, window: &Window, handle: WindowHandle) -> Self {
        let latest = Arc::new(Mutex::new(None));
        let ids = Arc::new(Mutex::new(BTreeMap::new()));
        let (action_tx, action_rx) = mpsc::channel();
        let adapter = Adapter::with_direct_handlers(
            event_loop,
            window,
            InitialTreeHandler {
                latest: latest.clone(),
            },
            RequestHandler {
                window: handle,
                ids: ids.clone(),
                tx: action_tx,
            },
            DeactivateHandler,
        );
        Self {
            adapter,
            latest,
            ids,
            action_rx,
        }
    }

    pub(crate) fn process_event(&mut self, window: &Window, event: &WindowEvent) {
        self.adapter.process_event(window, event);
    }

    pub(crate) fn update(&mut self, snapshot: &UiSemanticSnapshot) -> Result<(), PlatformError> {
        snapshot.validate().map_err(|error| {
            PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "accessibility.windows.update",
                format!("semantic snapshot validation failed: {error}"),
            )
        })?;
        let (update, ids) = tree_update(snapshot)?;
        *self.ids.lock().map_err(|_| poisoned())? = ids;
        *self.latest.lock().map_err(|_| poisoned())? = Some(update.clone());
        self.adapter.update_if_active(|| update);
        Ok(())
    }

    pub(crate) fn drain_actions(&mut self) -> Vec<AccessibilityActionRequest> {
        self.action_rx.try_iter().collect()
    }
}

fn tree_update(
    snapshot: &UiSemanticSnapshot,
) -> Result<(TreeUpdate, BTreeMap<NodeId, String>), PlatformError> {
    let mut semantic_to_native = BTreeMap::new();
    let mut native_to_semantic = BTreeMap::new();
    for (index, node) in snapshot.nodes.iter().enumerate() {
        let raw = u64::try_from(index)
            .ok()
            .and_then(|index| index.checked_add(1))
            .ok_or_else(|| accessibility_error("semantic node id overflow"))?;
        let id = NodeId(raw);
        semantic_to_native.insert(node.id.as_str(), id);
        native_to_semantic.insert(id, node.id.clone());
    }
    let root = *semantic_to_native
        .get(snapshot.root_id.as_str())
        .ok_or_else(|| accessibility_error("semantic root is absent"))?;
    let mut children = BTreeMap::<&str, Vec<NodeId>>::new();
    for node in &snapshot.nodes {
        if let Some(parent) = node.parent_id.as_deref() {
            let child = *semantic_to_native
                .get(node.id.as_str())
                .ok_or_else(|| accessibility_error("semantic child is absent"))?;
            children.entry(parent).or_default().push(child);
        }
    }
    let mut nodes = Vec::with_capacity(snapshot.nodes.len());
    let mut focus = root;
    for semantic in &snapshot.nodes {
        let id = *semantic_to_native
            .get(semantic.id.as_str())
            .ok_or_else(|| accessibility_error("semantic node is absent"))?;
        let mut node = Node::new(role(semantic.role));
        if let Some(name) = semantic.name.as_deref() {
            node.set_label(name);
        }
        if let Some(description) = semantic.description.as_deref() {
            node.set_description(description);
        }
        if let Some(value) = semantic.value.as_deref() {
            node.set_value(value);
        }
        if semantic.role == UiSemanticRole::Slider {
            node.set_numeric_value(semantic_property_number(semantic, "range.value")?);
            node.set_min_numeric_value(semantic_property_number(semantic, "range.min")?);
            node.set_max_numeric_value(semantic_property_number(semantic, "range.max")?);
            node.set_numeric_value_step(semantic_property_number(semantic, "range.step")?);
        }
        node.set_bounds(Rect {
            x0: semantic.bounds_points.min.x as f64,
            y0: semantic.bounds_points.min.y as f64,
            x1: semantic.bounds_points.max.x as f64,
            y1: semantic.bounds_points.max.y as f64,
        });
        if let Some(node_children) = children.remove(semantic.id.as_str()) {
            node.set_children(node_children);
        }
        if !semantic.enabled {
            node.set_disabled();
        }
        if semantic.hidden {
            node.set_hidden();
        }
        if semantic.selected {
            node.set_selected(true);
        }
        if let Some(checked) = semantic.checked {
            node.set_toggled(Toggled::from(checked));
        }
        if semantic.focused {
            focus = id;
        }
        for action in &semantic.actions {
            node.add_action(native_action(*action));
        }
        nodes.push((id, node));
    }
    let mut tree = Tree::new(root);
    tree.toolkit_name = Some("Astra UI".to_string());
    tree.toolkit_version = Some("1".to_string());
    Ok((
        TreeUpdate {
            nodes,
            tree: Some(tree),
            tree_id: TreeId::ROOT,
            focus,
        },
        native_to_semantic,
    ))
}

fn role(role: UiSemanticRole) -> Role {
    match role {
        UiSemanticRole::Application => Role::Application,
        UiSemanticRole::Window => Role::Window,
        UiSemanticRole::Dialog => Role::Dialog,
        UiSemanticRole::Group => Role::Group,
        UiSemanticRole::Text => Role::Label,
        UiSemanticRole::Image => Role::Image,
        UiSemanticRole::Button => Role::Button,
        UiSemanticRole::Toggle => Role::CheckBox,
        UiSemanticRole::Slider => Role::Slider,
        UiSemanticRole::Select => Role::ComboBox,
        UiSemanticRole::List => Role::List,
        UiSemanticRole::ListItem => Role::ListItem,
        UiSemanticRole::Grid => Role::Grid,
        UiSemanticRole::GridCell => Role::Cell,
        UiSemanticRole::TextInput => Role::TextInput,
        UiSemanticRole::Link => Role::Link,
        UiSemanticRole::Canvas => Role::Canvas,
    }
}

fn native_action(action: UiSemanticAction) -> Action {
    match action {
        UiSemanticAction::Focus => Action::Focus,
        UiSemanticAction::Activate | UiSemanticAction::Dismiss => Action::Click,
        UiSemanticAction::Increment => Action::Increment,
        UiSemanticAction::Decrement => Action::Decrement,
        UiSemanticAction::SetValue => Action::SetValue,
        UiSemanticAction::ScrollIntoView => Action::ScrollIntoView,
    }
}

fn action_name(action: Action) -> Option<&'static str> {
    match action {
        Action::Click => Some("invoke"),
        Action::Focus => Some("focus"),
        Action::Increment => Some("increment"),
        Action::Decrement => Some("decrement"),
        Action::SetValue | Action::ReplaceSelectedText => Some("set_value"),
        Action::ScrollIntoView => Some("scroll_into_view"),
        _ => None,
    }
}

fn accessibility_error(message: &'static str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::IntegrityMismatch,
        "accessibility.windows.update",
        message,
    )
}

fn semantic_property_number(
    node: &astra_ui_core::UiSemanticNode,
    key: &str,
) -> Result<f64, PlatformError> {
    node.properties
        .get(key)
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .ok_or_else(|| accessibility_error("semantic range metadata is invalid"))
}

fn poisoned() -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidState,
        "accessibility.windows.update",
        "accessibility bridge state was poisoned",
    )
}
