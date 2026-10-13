use std::collections::{BTreeMap, BTreeSet};

use astra_core::{DiagnosticSeverity, Hash256, SourceRef};

use astra_ui_core::{
    UiBindingManifest, UiBindingRoot, UiBlueprintBundle, UiCapability, UiEventBinding,
    UiNodeBlueprint, UiProfileScopedBindings, UiRepeatBinding, UiThemeManifest, UiValue,
    UiValueExpr, UiViewBinding, UiViewBlueprint, ValidateUi,
};

use crate::lower::{lower_sources_from_cst, ParsedLine};

use crate::{
    AstraSource, CompiledStory, CompiledVnProject, ReadingMode, SaveCompletionPolicy,
    SystemPageKind, SystemUiProfilePolicy, VnError,
};

mod compile_project_ui;
pub(crate) use compile_project_ui::compile_project_ui;
mod validate_node_bindings;
use validate_node_bindings::binding_key;
use validate_node_bindings::parse_binding;
use validate_node_bindings::parse_node;
use validate_node_bindings::validate_node_bindings;
mod validate_widget_properties;
use validate_widget_properties::diagnostic;
use validate_widget_properties::hash_bindings;
use validate_widget_properties::hash_blueprints;
use validate_widget_properties::hash_project;
use validate_widget_properties::infer_capabilities;
use validate_widget_properties::required_argument;
use validate_widget_properties::required_attr;
use validate_widget_properties::ui_error;
use validate_widget_properties::validate_action;
use validate_widget_properties::validate_widget_event;
use validate_widget_properties::validate_widget_properties;
