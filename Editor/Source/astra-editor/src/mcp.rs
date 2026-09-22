use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router, ServerHandler, ServiceExt,
};

use crate::bridge::EditorBridge;

#[derive(Clone)]
pub struct EditorMcp {
    bridge: EditorBridge,
    tool_router: ToolRouter<Self>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ApplyArgs {
    /// JSON EditBatch: documents [{path,version,edits:[{start,end,replacement}]}]. UTF-8 offsets.
    batch: serde_json::Value,
    /// Generation returned by read_document; cancellation invalidates it.
    generation: u64,
}

#[tool_router]
impl EditorMcp {
    pub fn new(bridge: EditorBridge) -> Self {
        Self {
            bridge,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Read the active unsaved Astra source and its version and agent generation."
    )]
    async fn read_document(&self) -> String {
        match self.bridge.read().await {
            Ok(document) => {
                serde_json::json!({"document":document,"generation":self.bridge.generation()})
                    .to_string()
            }
            Err(error) => serde_json::json!({"error":error.to_string()}).to_string(),
        }
    }

    #[tool(
        description = "Atomically apply an edit batch to the open Editor, with version conflict checks and one batch undo. Review mode waits for the user's decision."
    )]
    async fn apply_batch(
        &self,
        Parameters(args): Parameters<ApplyArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let batch = serde_json::from_value(args.batch)
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        match self.bridge.apply(args.generation, batch).await {
            Ok(()) => Ok(CallToolResult::success(vec![ContentBlock::text("Applied")])),
            Err(error) => Ok(CallToolResult::error(vec![ContentBlock::text(
                error.to_string(),
            )])),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for EditorMcp {
    fn get_info(&self) -> ServerConfig {
        {
            let mut config = ServerConfig::default();
            config.capabilities = ServerCapabilities::builder().enable_tools().build();
            config
        }
    }
}

pub async fn serve(bridge: EditorBridge) -> anyhow::Result<()> {
    EditorMcp::new(bridge)
        .serve(rmcp::transport::stdio())
        .await?
        .waiting()
        .await?;
    Ok(())
}
