use astra_ui_core::{UiBackendDescriptor, UiBindingManifest, UiViewBinding};
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum VnUiBindingError {
    #[error("ASTRA_VN_UI_BINDING_MISSING: no UI binding resolves for the current surface")]
    Missing,
    #[error("ASTRA_VN_UI_CAPABILITY_MISSING: selected provider lacks required capability")]
    Capability,
}

pub struct VnUiBindingRequest<'a> {
    pub command_id: Option<&'a str>,
    pub system_page: Option<&'a str>,
    pub surface: Option<&'a str>,
    pub profile: &'a str,
}

pub fn resolve_binding<'a>(
    manifest: &'a UiBindingManifest,
    request: VnUiBindingRequest<'_>,
) -> Result<&'a UiViewBinding, VnUiBindingError> {
    let scoped = manifest.profile_scoped_bindings.get(request.profile);
    request
        .command_id
        .and_then(|id| scoped.and_then(|bindings| bindings.command_bindings.get(id)))
        .or_else(|| {
            request
                .system_page
                .and_then(|id| scoped.and_then(|bindings| bindings.system_page_bindings.get(id)))
        })
        .or_else(|| {
            request
                .surface
                .and_then(|id| scoped.and_then(|bindings| bindings.surface_bindings.get(id)))
        })
        .or_else(|| {
            request
                .command_id
                .and_then(|id| manifest.command_bindings.get(id))
        })
        .or_else(|| {
            request
                .system_page
                .and_then(|id| manifest.system_page_bindings.get(id))
        })
        .or_else(|| {
            request
                .surface
                .and_then(|id| manifest.surface_bindings.get(id))
        })
        .or_else(|| manifest.profile_bindings.get(request.profile))
        .ok_or(VnUiBindingError::Missing)
}

pub fn validate_provider_for_view(
    backend: &UiBackendDescriptor,
    required: &[astra_ui_core::UiCapability],
) -> Result<(), VnUiBindingError> {
    if required
        .iter()
        .any(|capability| !backend.capabilities.contains(capability))
    {
        return Err(VnUiBindingError::Capability);
    }
    Ok(())
}
