use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use astra_vn_editor::{DocumentSnapshot, EditBatch};
use tokio::sync::{mpsc, oneshot};

pub enum Request {
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
