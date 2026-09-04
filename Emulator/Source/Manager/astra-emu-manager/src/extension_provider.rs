use std::sync::Arc;

use astra_byte_source::OwnedByteBuffer;
use astra_emu_extension_api::{
    ConfigSchemaRequestV1, ConfigSchemaResponseV1, FilterGraphRequestV1, FilterGraphResponseV1,
    CONFIG_SCHEMA_HOOK_ID, FILTER_GRAPH_HOOK_ID, TRANSLATION_TEXT_HOOK_ID,
};
use astra_emu_family_api::{
    LegacyDiagnostic, LegacyHookInvocationV1, LegacyHookResultV1, LegacyHookStatusV1,
    LegacyProviderError,
};
use astra_emu_manager_core::{
    extension_config_schema, family_config_schema, filter_config_schema, FilterGraph,
    SynchronousFamilyHookProvider,
};

pub struct AstraEmuExtensionProvider {
    translation: Option<Arc<dyn SynchronousFamilyHookProvider>>,
    // No extra state needed for filter/config; they are stateless.
}

impl AstraEmuExtensionProvider {
    pub fn new(translation: Option<Arc<dyn SynchronousFamilyHookProvider>>) -> Self {
        Self { translation }
    }
}

impl SynchronousFamilyHookProvider for AstraEmuExtensionProvider {
    fn provider_id(&self) -> &str {
        "astra.emu.extension.composite.v1"
    }

    fn invoke(
        &self,
        invocation: &LegacyHookInvocationV1,
    ) -> Result<LegacyHookResultV1, LegacyProviderError> {
        match invocation.hook_id.as_str() {
            TRANSLATION_TEXT_HOOK_ID => {
                if let Some(provider) = &self.translation {
                    provider.invoke(invocation)
                } else {
                    Ok(unbound_hook("translation provider not bound"))
                }
            }
            FILTER_GRAPH_HOOK_ID => handle_filter_graph(invocation),
            CONFIG_SCHEMA_HOOK_ID => handle_config_schema(invocation),
            _ => Err(LegacyProviderError::invalid(
                "ASTRA_EMU_EXTENSION_HOOK_ID",
                format!("unsupported hook {}", invocation.hook_id),
            )),
        }
    }
}

fn handle_filter_graph(
    invocation: &LegacyHookInvocationV1,
) -> Result<LegacyHookResultV1, LegacyProviderError> {
    let request: FilterGraphRequestV1 = postcard::from_bytes(&invocation.payload).map_err(|e| {
        LegacyProviderError::invalid(
            "ASTRA_EMU_FILTER_GRAPH_REQUEST_DECODE",
            format!("filter graph request decode failed: {e}"),
        )
    })?;
    request
        .validate()
        .map_err(|e| LegacyProviderError::invalid("ASTRA_EMU_FILTER_GRAPH_REQUEST_VALIDATE", e))?;
    let graph = FilterGraph::for_preset(&request.preset_id);
    let graph_json = postcard::to_allocvec(&graph).map_err(|e| {
        LegacyProviderError::invalid(
            "ASTRA_EMU_FILTER_GRAPH_RESPONSE_ENCODE",
            format!("filter graph encode failed: {e}"),
        )
    })?;
    let response = FilterGraphResponseV1 { graph_json };
    let payload = postcard::to_allocvec(&response).map_err(|e| {
        LegacyProviderError::invalid(
            "ASTRA_EMU_FILTER_GRAPH_RESPONSE_ENCODE",
            format!("filter graph response encode failed: {e}"),
        )
    })?;
    Ok(LegacyHookResultV1 {
        status: LegacyHookStatusV1::Completed,
        payload: OwnedByteBuffer::from_vec(payload),
        diagnostics: Vec::new(),
    })
}

fn handle_config_schema(
    invocation: &LegacyHookInvocationV1,
) -> Result<LegacyHookResultV1, LegacyProviderError> {
    let request: ConfigSchemaRequestV1 =
        postcard::from_bytes(&invocation.payload).map_err(|e| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_CONFIG_SCHEMA_REQUEST_DECODE",
                format!("config schema request decode failed: {e}"),
            )
        })?;
    request
        .validate()
        .map_err(|e| LegacyProviderError::invalid("ASTRA_EMU_CONFIG_SCHEMA_REQUEST_VALIDATE", e))?;
    let owner_id = request.owner_id.as_str();
    let schema = if owner_id == "filter" {
        Some(filter_config_schema())
    } else {
        family_config_schema(owner_id)
            .or_else(|| extension_config_schema(owner_id))
    };
    let Some(schema) = schema else {
        return Ok(LegacyHookResultV1 {
            status: LegacyHookStatusV1::Failed,
            payload: OwnedByteBuffer::from_vec(Vec::new()),
            diagnostics: vec![LegacyDiagnostic {
                code: "ASTRA_EMU_CONFIG_SCHEMA_NOT_FOUND".into(),
                severity: "warn".into(),
                message: format!("config schema not found for owner {owner_id}"),
                subject: "config".into(),
            }],
        });
    };
    let schema_json = postcard::to_allocvec(&schema).map_err(|e| {
        LegacyProviderError::invalid(
            "ASTRA_EMU_CONFIG_SCHEMA_RESPONSE_ENCODE",
            format!("config schema encode failed: {e}"),
        )
    })?;
    let response = ConfigSchemaResponseV1 { schema_json };
    let payload = postcard::to_allocvec(&response).map_err(|e| {
        LegacyProviderError::invalid(
            "ASTRA_EMU_CONFIG_SCHEMA_RESPONSE_ENCODE",
            format!("config schema response encode failed: {e}"),
        )
    })?;
    Ok(LegacyHookResultV1 {
        status: LegacyHookStatusV1::Completed,
        payload: OwnedByteBuffer::from_vec(payload),
        diagnostics: Vec::new(),
    })
}

fn unbound_hook(message: &str) -> LegacyHookResultV1 {
    LegacyHookResultV1 {
        status: LegacyHookStatusV1::Unbound,
        payload: OwnedByteBuffer::from_vec(Vec::new()),
        diagnostics: vec![LegacyDiagnostic {
            code: "ASTRA_EMU_EXTENSION_UNBOUND".into(),
            severity: "warn".into(),
            message: message.into(),
            subject: "extension".into(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_emu_extension_api::{CONFIG_SCHEMA_HOOK_ID, FILTER_GRAPH_HOOK_ID};

    fn invoke_config(owner_id: &str) -> LegacyHookResultV1 {
        let provider = AstraEmuExtensionProvider::new(None);
        let request = ConfigSchemaRequestV1 {
            owner_id: owner_id.into(),
        };
        let payload = postcard::to_allocvec(&request).unwrap();
        let invocation = LegacyHookInvocationV1 {
            session_id: "test-session".into(),
            invocation_id: "invocation-1".into(),
            family_id: "fvp".into(),
            family_game_id: "game-1".into(),
            hook_id: CONFIG_SCHEMA_HOOK_ID.into(),
            timeout_ms: 2000,
            payload: OwnedByteBuffer::from_vec(payload),
        };
        provider.invoke(&invocation).unwrap()
    }

    #[test]
    fn config_schema_hook_returns_family_and_filter_schemas() {
        let result = invoke_config("fvp");
        assert_eq!(result.status, LegacyHookStatusV1::Completed);
        let response: ConfigSchemaResponseV1 = postcard::from_bytes(&result.payload).unwrap();
        let schema: astra_emu_manager_core::ConfigSchema =
            postcard::from_bytes(&response.schema_json).unwrap();
        assert_eq!(schema.owner_id, "fvp");

        let result = invoke_config("filter");
        assert_eq!(result.status, LegacyHookStatusV1::Completed);
        let response: ConfigSchemaResponseV1 = postcard::from_bytes(&result.payload).unwrap();
        let schema: astra_emu_manager_core::ConfigSchema =
            postcard::from_bytes(&response.schema_json).unwrap();
        assert_eq!(schema.owner_id, "filter");
    }

    #[test]
    fn filter_graph_hook_returns_graph_for_preset() {
        let provider = AstraEmuExtensionProvider::new(None);
        let request = FilterGraphRequestV1 {
            preset_id: "grayscale".into(),
            layer: "final".into(),
        };
        let payload = postcard::to_allocvec(&request).unwrap();
        let invocation = LegacyHookInvocationV1 {
            session_id: "test-session".into(),
            invocation_id: "invocation-2".into(),
            family_id: "fvp".into(),
            family_game_id: "game-1".into(),
            hook_id: FILTER_GRAPH_HOOK_ID.into(),
            timeout_ms: 2000,
            payload: OwnedByteBuffer::from_vec(payload),
        };
        let result = provider.invoke(&invocation).unwrap();
        assert_eq!(result.status, LegacyHookStatusV1::Completed);
        let response: FilterGraphResponseV1 = postcard::from_bytes(&result.payload).unwrap();
        let graph: astra_emu_manager_core::FilterGraph =
            postcard::from_bytes(&response.graph_json).unwrap();
        assert_eq!(graph.bindings.len(), 1);
        assert_eq!(graph.bindings[0].preset_id, "grayscale");
    }

    #[test]
    fn translate_hook_delegates_or_unbound() {
        let provider = AstraEmuExtensionProvider::new(None);
        let invocation = LegacyHookInvocationV1 {
            session_id: "test-session".into(),
            invocation_id: "invocation-3".into(),
            family_id: "fvp".into(),
            family_game_id: "game-1".into(),
            hook_id: TRANSLATION_TEXT_HOOK_ID.into(),
            timeout_ms: 2000,
            payload: OwnedByteBuffer::from_vec(b"hello".to_vec()),
        };
        let result = provider.invoke(&invocation).unwrap();
        assert_eq!(result.status, LegacyHookStatusV1::Unbound);
    }
}
