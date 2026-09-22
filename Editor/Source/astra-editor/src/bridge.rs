use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use astra_vn_editor::{DocumentSnapshot, EditBatch};
use tokio::sync::{mpsc, oneshot};

pub enum Request {
    Permission {
        generation: u64,
        request: Box<agent_client_protocol::schema::v1::RequestPermissionRequest>,
        reply: oneshot::Sender<agent_client_protocol::schema::v1::RequestPermissionOutcome>,
    },
    AgentMessage {
        generation: u64,
        text: String,
    },
    Read {
        reply: oneshot::Sender<anyhow::Result<DocumentSnapshot>>,
    },
    Apply {
        generation: u64,
        batch: EditBatch,
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
}

#[derive(Clone)]
pub struct EditorBridge {
    sender: mpsc::Sender<Request>,
    generation: Arc<AtomicU64>,
}

impl EditorBridge {
    pub async fn permission(
        &self,
        generation: u64,
        request: agent_client_protocol::schema::v1::RequestPermissionRequest,
    ) -> agent_client_protocol::schema::v1::RequestPermissionOutcome {
        use agent_client_protocol::schema::v1::RequestPermissionOutcome;
        if generation != self.generation() {
            return RequestPermissionOutcome::Cancelled;
        }
        let (reply, receive) = oneshot::channel();
        if self
            .sender
            .send(Request::Permission {
                generation,
                request: Box::new(request),
                reply,
            })
            .await
            .is_err()
        {
            return RequestPermissionOutcome::Cancelled;
        }
        let outcome = receive.await.unwrap_or(RequestPermissionOutcome::Cancelled);
        if generation == self.generation() {
            outcome
        } else {
            RequestPermissionOutcome::Cancelled
        }
    }
    pub async fn message(&self, generation: u64, text: String) {
        let _ = self
            .sender
            .send(Request::AgentMessage { generation, text })
            .await;
    }
    pub fn new() -> (Self, mpsc::Receiver<Request>) {
        let (sender, receiver) = mpsc::channel(16);
        (
            Self {
                sender,
                generation: Arc::new(AtomicU64::new(1)),
            },
            receiver,
        )
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
    pub fn cancel(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    pub async fn read(&self) -> anyhow::Result<DocumentSnapshot> {
        let (reply, receive) = oneshot::channel();
        self.sender.send(Request::Read { reply }).await?;
        receive.await?
    }

    pub async fn apply(&self, generation: u64, batch: EditBatch) -> anyhow::Result<()> {
        anyhow::ensure!(generation == self.generation(), "Cancelled agent request");
        let (reply, receive) = oneshot::channel();
        self.sender
            .send(Request::Apply {
                generation,
                batch,
                reply,
            })
            .await?;
        receive.await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{
        RequestPermissionOutcome, RequestPermissionRequest, SelectedPermissionOutcome,
        ToolCallUpdate,
    };

    #[test]
    fn permission_approval_after_cancellation_is_rejected() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (bridge, mut receiver) = EditorBridge::new();
            let caller = bridge.clone();
            let task = tokio::spawn(async move {
                caller
                    .permission(
                        1,
                        RequestPermissionRequest::new(
                            "test",
                            ToolCallUpdate::new("call", Default::default()),
                            vec![],
                        ),
                    )
                    .await
            });
            let Request::Permission { reply, .. } = receiver.recv().await.unwrap() else {
                panic!("permission request");
            };
            bridge.cancel();
            reply
                .send(RequestPermissionOutcome::Selected(
                    SelectedPermissionOutcome::new("allow"),
                ))
                .unwrap();
            assert_eq!(task.await.unwrap(), RequestPermissionOutcome::Cancelled);
        });
    }
}
