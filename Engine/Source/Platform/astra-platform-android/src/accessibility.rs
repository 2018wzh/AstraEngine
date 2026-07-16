#![cfg(target_os = "android")]

use std::{
    collections::BTreeMap,
    mem::ManuallyDrop,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
};

use accesskit::{
    Action, ActionData, ActionHandler, ActionRequest, ActivationHandler, Node, NodeId, Rect, Role,
    Toggled, Tree, TreeId, TreeUpdate,
};
use accesskit_android::{
    jni::{objects::JObject, JavaVM},
    InjectingAdapter,
};
use android_activity::AndroidApp;
use astra_platform::{PlatformError, PlatformErrorCode, WindowHandle};
use astra_ui_core::{UiSemanticAction, UiSemanticRole, UiSemanticSnapshot, ValidateUi};

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
    tx: mpsc::SyncSender<AccessibilityActionRequest>,
    overflow_count: Arc<AtomicU64>,
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
        if self
            .tx
            .try_send(AccessibilityActionRequest {
                window: self.window,
                semantic_id,
                action: action.to_string(),
                value,
            })
            .is_err()
        {
            self.overflow_count.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub(crate) struct AndroidAccessibilityBridge {
    adapter: InjectingAdapter,
    latest: Arc<Mutex<Option<TreeUpdate>>>,
    ids: Arc<Mutex<BTreeMap<NodeId, String>>>,
    action_rx: mpsc::Receiver<AccessibilityActionRequest>,
    overflow_count: Arc<AtomicU64>,
}

impl AndroidAccessibilityBridge {
    pub(crate) fn new(
        app: &AndroidApp,
        window: WindowHandle,
        action_capacity: usize,
    ) -> Result<Self, PlatformError> {
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }
            .map_err(|_| accessibility_state_error("Java VM is unavailable"))?;
        let mut env = vm
            .attach_current_thread()
            .map_err(|_| accessibility_state_error("Java thread attachment failed"))?;
        let activity = ManuallyDrop::new(unsafe {
            JObject::from_raw(app.activity_as_ptr() as accesskit_android::jni::sys::jobject)
        });
        let window_object = env
            .call_method(&*activity, "getWindow", "()Landroid/view/Window;", &[])
            .and_then(|value| value.l())
            .map_err(|_| accessibility_state_error("Activity window is unavailable"))?;
        let decor_view = env
            .call_method(&window_object, "getDecorView", "()Landroid/view/View;", &[])
            .and_then(|value| value.l())
            .map_err(|_| accessibility_state_error("Activity decor view is unavailable"))?;
        let latest = Arc::new(Mutex::new(None));
        let ids = Arc::new(Mutex::new(BTreeMap::new()));
        let (action_tx, action_rx) = mpsc::sync_channel(action_capacity.max(1));
        let overflow_count = Arc::new(AtomicU64::new(0));
        let adapter = InjectingAdapter::new(
            &mut env,
            &decor_view,
            InitialTreeHandler {
                latest: Arc::clone(&latest),
            },
            RequestHandler {
                window,
                ids: Arc::clone(&ids),
                tx: action_tx,
                overflow_count: Arc::clone(&overflow_count),
            },
        );
        Ok(Self {
            adapter,
            latest,
            ids,
            action_rx,
            overflow_count,
        })
    }

    pub(crate) fn update(&mut self, snapshot: &UiSemanticSnapshot) -> Result<(), PlatformError> {
        snapshot
            .validate()
            .map_err(|_| accessibility_integrity_error("semantic snapshot validation failed"))?;
        let (update, ids) = tree_update(snapshot)?;
        *self.ids.lock().map_err(|_| poisoned())? = ids;
        *self.latest.lock().map_err(|_| poisoned())? = Some(update.clone());
        self.adapter.update_if_active(|| update);
        Ok(())
    }

    pub(crate) fn drain_actions(
        &mut self,
    ) -> Result<Vec<AccessibilityActionRequest>, PlatformError> {
        let dropped_count = self.overflow_count.swap(0, Ordering::AcqRel);
        if dropped_count != 0 {
            tracing::error!(
                event = "platform.android.accessibility.queue_overflow",
                diagnostic_code = "ASTRA_ANDROID_ACCESSIBILITY_QUEUE_OVERFLOW",
                dropped_count,
                "Android accessibility action queue overflowed"
            );
            while self.action_rx.try_recv().is_ok() {}
            return Err(PlatformError::new(
                PlatformErrorCode::QueueOverflow,
                "accessibility.android.drain",
                "Android accessibility action queue overflowed",
            ));
        }
        Ok(self.action_rx.try_iter().collect())
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
            .ok_or_else(|| accessibility_integrity_error("semantic node id overflow"))?;
        let id = NodeId(raw);
        semantic_to_native.insert(node.id.as_str(), id);
        native_to_semantic.insert(id, node.id.clone());
    }
    let root = *semantic_to_native
        .get(snapshot.root_id.as_str())
        .ok_or_else(|| accessibility_integrity_error("semantic root is absent"))?;
    let mut children = BTreeMap::<&str, Vec<NodeId>>::new();
    for node in &snapshot.nodes {
        if let Some(parent) = node.parent_id.as_deref() {
            let child = *semantic_to_native
                .get(node.id.as_str())
                .ok_or_else(|| accessibility_integrity_error("semantic child is absent"))?;
            children.entry(parent).or_default().push(child);
        }
    }
    let mut nodes = Vec::with_capacity(snapshot.nodes.len());
    let mut focus = root;
    for semantic in &snapshot.nodes {
        let id = *semantic_to_native
            .get(semantic.id.as_str())
            .ok_or_else(|| accessibility_integrity_error("semantic node is absent"))?;
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

fn semantic_property_number(
    node: &astra_ui_core::UiSemanticNode,
    key: &str,
) -> Result<f64, PlatformError> {
    node.properties
        .get(key)
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .ok_or_else(|| accessibility_integrity_error("semantic range metadata is invalid"))
}

fn accessibility_integrity_error(message: &'static str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::IntegrityMismatch,
        "accessibility.android.update",
        message,
    )
}

fn accessibility_state_error(message: &'static str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidState,
        "accessibility.android.initialize",
        message,
    )
}

fn poisoned() -> PlatformError {
    accessibility_state_error("accessibility bridge state was poisoned")
}
