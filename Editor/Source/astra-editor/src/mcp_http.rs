use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use tokio_util::sync::CancellationToken;

use crate::{bridge::EditorBridge, mcp::EditorMcp};

/// A turn-scoped MCP endpoint; never listens outside loopback or persists credentials.
pub(crate) struct AgentMcp {
    pub url: String,
    pub authorization: String,
    cancel: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

async fn authorize(
    State(expected): State<String>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        != Some(expected.as_str())
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(request).await)
}

impl AgentMcp {
    pub async fn start(bridge: EditorBridge) -> anyhow::Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let authorization = format!("Bearer {}", uuid::Uuid::new_v4());
        let cancel = CancellationToken::new();
        let handler = EditorMcp::for_turn(bridge);
        let config = StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .enforce_origin_validation()
            .with_max_request_body_bytes(16 * 1024 * 1024)
            .with_cancellation_token(cancel.child_token());
        let service = StreamableHttpService::new(
            move || Ok(handler.clone()),
            Arc::new(LocalSessionManager::default()),
            config,
        );
        let router = axum::Router::new().nest_service("/mcp", service).layer(
            middleware::from_fn_with_state(authorization.clone(), authorize),
        );
        let shutdown = cancel.clone();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await;
        });
        Ok(Self {
            url: format!("http://{address}/mcp"),
            authorization,
            cancel,
            task,
        })
    }

    pub async fn shutdown(&mut self) {
        self.cancel.cancel();
        if tokio::time::timeout(std::time::Duration::from_secs(2), &mut self.task)
            .await
            .is_err()
        {
            self.task.abort();
            let _ = (&mut self.task).await;
        }
    }
}

impl Drop for AgentMcp {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn turn_endpoint_rejects_unauthenticated_requests_and_closes() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (bridge, _) = EditorBridge::new();
            let mut server = AgentMcp::start(bridge).await.unwrap();
            let address = server.url.strip_prefix("http://").unwrap().strip_suffix("/mcp").unwrap().to_string();
            let mut stream = tokio::net::TcpStream::connect(&address).await.unwrap();
            stream.write_all(format!("POST /mcp HTTP/1.1\r\nHost: {address}\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}").as_bytes()).await.unwrap();
            let mut response = Vec::new();
            tokio::time::timeout(std::time::Duration::from_secs(2), stream.read_to_end(&mut response)).await.unwrap().unwrap();
            assert!(String::from_utf8(response).unwrap().starts_with("HTTP/1.1 401"));
            server.shutdown().await;
            assert!(tokio::net::TcpStream::connect(&address).await.is_err());
        });
    }
}
