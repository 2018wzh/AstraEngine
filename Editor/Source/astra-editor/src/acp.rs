use std::{path::PathBuf, str::FromStr, sync::Arc};

use agent_client_protocol::{
    schema::{v1::*, ProtocolVersion},
    AcpAgent, Agent, ConnectionTo,
};
use tokio::sync::Mutex;

use crate::bridge::EditorBridge;

/// External agent owns model credentials/configuration. Only the open document is exposed.
pub async fn prompt(
    command: String,
    prompt: String,
    source: PathBuf,
    bridge: EditorBridge,
) -> anyhow::Result<()> {
    let source = source.canonicalize()?;
    let prompt = format!("Edit only the open source using the astra_editor MCP server read_document and apply_batch tools. Read the current document and generation first; submit UTF-8 byte edits with its exact version and generation. Changes are reviewed in the Editor. Do not write source files directly or use shell commands.\n\n{}", prompt);
    let root = source
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Source has no parent"))?
        .to_path_buf();
    let generation = bridge.generation();
    let notification_bridge = bridge.clone();
    let permission_bridge = bridge.clone();
    let calls = Arc::new(Mutex::new(
        std::collections::BTreeMap::<String, ToolCall>::new(),
    ));
    let permission_calls = calls.clone();
    let agent = AcpAgent::from_str(&command)?;
    let cancellation_bridge = bridge.clone();
    let mut mcp = crate::mcp_http::AgentMcp::start(bridge.clone()).await?;
    let mcp_server = McpServer::Http(McpServerHttp::new("astra_editor", mcp.url.clone()).headers(
        vec![HttpHeader::new("Authorization", mcp.authorization.clone())],
    ));
    let connection = agent_client_protocol::Client.builder()
        .on_receive_notification(async move |notification: SessionNotification, _cx| {
            match notification.update {
                SessionUpdate::AgentMessageChunk(chunk) => {
                    if let ContentBlock::Text(text) = chunk.content { notification_bridge.message(generation, text.text).await; }
                }
                SessionUpdate::ToolCall(call) => {
                    let mut calls = calls.lock().await;
                    if calls.len() >= 64 { calls.pop_first(); }
                    calls.insert(call.tool_call_id.to_string(), call);
                }
                SessionUpdate::ToolCallUpdate(update) => {
                    if let Some(call) = calls.lock().await.get_mut(&update.tool_call_id.to_string()) {
                        if let Some(title) = update.fields.title { call.title = title; }
                        if let Some(input) = update.fields.raw_input { call.raw_input = Some(input); }
                    }
                }
                _ => {}
            }
            Ok(())
        }, agent_client_protocol::on_receive_notification!())
        .on_receive_request(async move |mut request: RequestPermissionRequest, responder, _connection| {
            if let Some(call) = permission_calls.lock().await.get(&request.tool_call.tool_call_id.to_string()) {
                request.tool_call.fields.title.get_or_insert(call.title.clone());
                if request.tool_call.fields.raw_input.is_none() { request.tool_call.fields.raw_input = call.raw_input.clone(); }
            }
            responder.respond(RequestPermissionResponse::new(permission_bridge.permission(generation, request).await))
        }, agent_client_protocol::on_receive_request!())
        .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
            let initialized = connection.send_request(InitializeRequest::new(ProtocolVersion::V1).client_capabilities(ClientCapabilities::new())).block_task().await?;
            if !initialized.agent_capabilities.mcp_capabilities.http {
                return Err(agent_client_protocol::Error::new(-32600, "This ACP agent does not support HTTP MCP; choose an agent with that capability"));
            }
            let session = connection.send_request(NewSessionRequest::new(root).mcp_servers(vec![mcp_server])).block_task().await?.session_id;
            let request = connection.send_request(PromptRequest::new(session.clone(), vec![ContentBlock::Text(TextContent::new(prompt))])).block_task();
            tokio::pin!(request);
            loop {
                tokio::select! {
                    result = &mut request => { result?; break; }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {
                        if bridge.generation() != generation {
                            connection.send_notification(CancelNotification::new(session.clone()))?;
                            // Give the peer time to finish cancellation before closing its transport.
                            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), &mut request).await;
                            break;
                        }
                    }
                }
            }
            Ok(())
        });
    tokio::pin!(connection);
    loop {
        tokio::select! {
            result = &mut connection => { mcp.shutdown().await; result?; break; }
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {
                if cancellation_bridge.generation() != generation {
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), &mut connection).await;
                    mcp.shutdown().await;
                    break;
                }
            }
        }
    }
    Ok(())
}
