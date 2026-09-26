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
    generation: Option<u64>,
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
            generation: None,
        }
    }

    pub(crate) fn for_turn(bridge: EditorBridge) -> Self {
        let generation = Some(bridge.generation());
        Self {
            bridge,
            generation,
            tool_router: Self::tool_router(),
        }
    }

    fn active(&self) -> bool {
        self.generation
            .is_none_or(|generation| self.bridge.generation() == generation)
    }

    #[tool(
        description = "Read the active unsaved Astra source and its version and agent generation."
    )]
    async fn read_document(&self) -> String {
        if !self.active() {
            return serde_json::json!({"error":"Agent turn cancelled"}).to_string();
        }
        match self.bridge.read().await {
            Ok(document) if self.active() => {
                serde_json::json!({"timeline": astra_vn_editor::timeline::tracks(&document).map_err(|error| error.to_string()), "document":document,"generation":self.bridge.generation()})
                    .to_string()
            }
            Ok(_) => serde_json::json!({"error":"Agent turn cancelled"}).to_string(),
            Err(error) => serde_json::json!({"error":error.to_string()}).to_string(),
        }
    }

    #[tool(
        description = "Atomically apply an edit batch to the open Editor, with version conflict checks and one batch undo. Review mode waits for the user's decision."
    )]
    async fn apply_batch(
        &self,
        Parameters(args): Parameters<ApplyArgs>,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        if !self.active() {
            return Err(rmcp::ErrorData::invalid_request(
                "Agent turn cancelled",
                None,
            ));
        }
        let batch = serde_json::from_value(args.batch)
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        let applied = tokio::select! {
            result = self.bridge.apply(args.generation, batch) => result,
            _ = context.ct.cancelled() => Err(anyhow::anyhow!("MCP request cancelled")),
        };
        match applied {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn read_exposes_the_same_source_timeline_as_desktop() {
        let (bridge, mut requests) = EditorBridge::new();
        let server = EditorMcp::new(bridge);
        let reader = tokio::spawn(async move { server.read_document().await });
        let crate::bridge::Request::Read { reply } = requests.recv().await.unwrap() else {
            panic!("expected read")
        };
        reply.send(Ok(astra_vn_editor::DocumentSnapshot{path:"main.astra".into(),version:7,text:"timeline id:pan target:hero property:x keyframes:0=0,1000=1 easing:ease_out budget_ms:1 #@id pan\n".into()})).unwrap();
        let value: serde_json::Value = serde_json::from_str(&reader.await.unwrap()).unwrap();
        assert_eq!(value["document"]["version"], 7);
        assert_eq!(value["timeline"]["Ok"][0]["easing"], "ease_out");
        assert_eq!(value["timeline"]["Ok"][0]["source_id"], "pan");
    }
}
