use std::{path::PathBuf, str::FromStr, sync::Arc};

use agent_client_protocol::{
    schema::{v1::*, ProtocolVersion},
    AcpAgent, Agent, ConnectionTo,
};
use astra_vn_editor::{DocumentEdits, EditBatch, TextEdit};
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
    let prompt = format!("Edit only this open source through fs/read_text_file and fs/write_text_file: {}\nRead before each write; changes are reviewed in the Editor.\n\n{}", source.display(), prompt);
    let root = source
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Source has no parent"))?
        .to_path_buf();
    let generation = bridge.generation();
    let observed = Arc::new(Mutex::new(None::<astra_vn_editor::DocumentSnapshot>));
    let read_bridge = bridge.clone();
    let read_source = source.clone();
    let read_observed = observed.clone();
    let write_bridge = bridge.clone();
    let notification_bridge = bridge.clone();
    let agent = AcpAgent::from_str(&command)?;
    let cancellation_bridge = bridge.clone();
    let connection = agent_client_protocol::Client.builder()
        .on_receive_notification(async move |notification: SessionNotification, _cx| {
            if let SessionUpdate::AgentMessageChunk(chunk) = notification.update {
                if let ContentBlock::Text(text) = chunk.content { notification_bridge.message(generation, text.text).await; }
            }
            Ok(())
        }, agent_client_protocol::on_receive_notification!())
        .on_receive_request(async move |request: ReadTextFileRequest, responder, _connection| {
            let result = async {
                anyhow::ensure!(generation == read_bridge.generation(), "Cancelled turn");
                anyhow::ensure!(request.path.canonicalize()? == read_source, "Only the open source is available");
                let snapshot = read_bridge.read().await?;
                let content = if request.line.is_none() && request.limit.is_none() { snapshot.text.clone() } else {
                    snapshot.text.lines().skip(request.line.unwrap_or(1).saturating_sub(1) as usize)
                        .take(request.limit.unwrap_or(u32::MAX) as usize).collect::<Vec<_>>().join("\n")
                };
                *read_observed.lock().await = Some(snapshot);
                Ok::<_, anyhow::Error>(ReadTextFileResponse::new(content))
            }.await;
            match result { Ok(response) => responder.respond(response), Err(error) => responder.respond_with_internal_error(error) }
        }, agent_client_protocol::on_receive_request!())
        .on_receive_request(async move |request: WriteTextFileRequest, responder, _connection| {
            let result = async {
                anyhow::ensure!(request.path.canonicalize()? == source, "Only the open source is editable");
                let snapshot = observed.lock().await.take().ok_or_else(|| anyhow::anyhow!("Read the document before writing"))?;
                let batch = EditBatch { documents: vec![DocumentEdits { path: snapshot.path, version: snapshot.version,
                    edits: vec![TextEdit { start: 0, end: snapshot.text.len(), replacement: request.content }] }] };
                write_bridge.apply(generation, batch).await?;
                Ok::<_, anyhow::Error>(WriteTextFileResponse::new())
            }.await;
            match result { Ok(response) => responder.respond(response), Err(error) => responder.respond_with_internal_error(error) }
        }, agent_client_protocol::on_receive_request!())
        .on_receive_request(async move |_request: RequestPermissionRequest, responder, _connection| {
            // No shell/terminal authority is delegated. Source writes use the batch approval UI.
            responder.respond(RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled))
        }, agent_client_protocol::on_receive_request!())
        .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
            connection.send_request(InitializeRequest::new(ProtocolVersion::V1).client_capabilities(
                ClientCapabilities::new().fs(FileSystemCapabilities::new().read_text_file(true).write_text_file(true))
            )).block_task().await?;
            let session = connection.send_request(NewSessionRequest::new(root)).block_task().await?.session_id;
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
            result = &mut connection => { result?; break; }
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {
                if cancellation_bridge.generation() != generation {
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), &mut connection).await;
                    break;
                }
            }
        }
    }
    Ok(())
}
