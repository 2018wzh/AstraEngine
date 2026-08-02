//! Dedicated worker boundary for hosted RFVP sessions.
//!
//! RFVP's pack reader deliberately keeps a session-local, non-`Send` file
//! cursor. The dynamic family ABI, however, requires its provider object to
//! be `Send`. This executor creates that non-`Send` state on a dedicated
//! thread and permits only request/response closures to enter it. No session
//! state crosses the ABI boundary and no unsafe `Send` implementation is used.

use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
};

#[derive(Debug, thiserror::Error)]
pub enum HostedWorkerError {
    #[error("hosted session worker is unavailable")]
    Unavailable,
    #[error("hosted session worker terminated before answering")]
    Terminated,
    #[error("hosted session worker panicked")]
    Panicked,
}

#[derive(Debug, thiserror::Error)]
pub enum HostedWorkerStartError<E> {
    #[error("hosted session initialization failed: {0}")]
    Initialization(E),
    #[error(transparent)]
    Worker(#[from] HostedWorkerError),
}

enum WorkerCommand<T: 'static> {
    Execute(Box<dyn FnOnce(&mut T) + Send + 'static>),
    Shutdown,
}

/// A synchronous request port to one thread-confined hosted session.
///
/// `T` intentionally has no `Send` bound. It is constructed after the worker
/// starts and is never moved back to the calling thread. Callers can only send
/// `Send` closures and receive `Send` results, which makes the enclosing
/// dynamic provider safely `Send` without serializing or locking RFVP state.
pub struct HostedSessionWorker<T: 'static> {
    sender: Sender<WorkerCommand<T>>,
    join: Option<JoinHandle<()>>,
}

impl<T: 'static> HostedSessionWorker<T> {
    /// Runs a fallible request in the worker and preserves both transport and
    /// session errors without forcing provider code to nest reply channels.
    pub fn execute_result<R: Send + 'static, E: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut T) -> Result<R, E> + Send + 'static,
    ) -> Result<Result<R, E>, HostedWorkerError> {
        self.execute(operation)
    }
}

impl<T: 'static> HostedSessionWorker<T> {
    pub fn spawn(init: impl FnOnce() -> T + Send + 'static) -> Self {
        let (sender, receiver) = mpsc::channel();
        let join = thread::Builder::new()
            .name("astra-fvp-hosted-session".into())
            .spawn(move || run_worker(init, receiver))
            .expect("creating a hosted RFVP session worker must succeed");
        Self {
            sender,
            join: Some(join),
        }
    }

    /// Starts a worker and waits for its session-local initialization result.
    /// The initialized `T` still never leaves the worker thread.
    pub fn try_spawn<E: Send + 'static>(
        init: impl FnOnce() -> Result<T, E> + Send + 'static,
    ) -> Result<Self, HostedWorkerStartError<E>> {
        let (sender, receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let join = thread::Builder::new()
            .name("astra-fvp-hosted-session".into())
            .spawn(move || match init() {
                Ok(state) => {
                    let _ = ready_sender.send(Ok(()));
                    run_worker(|| state, receiver);
                }
                Err(error) => {
                    let _ = ready_sender.send(Err(error));
                }
            })
            .map_err(|_| HostedWorkerError::Unavailable)?;
        match ready_receiver
            .recv()
            .map_err(|_| HostedWorkerError::Terminated)?
        {
            Ok(()) => Ok(Self {
                sender,
                join: Some(join),
            }),
            Err(error) => {
                join.join().map_err(|_| HostedWorkerError::Panicked)?;
                Err(HostedWorkerStartError::Initialization(error))
            }
        }
    }

    pub fn execute<R: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut T) -> R + Send + 'static,
    ) -> Result<R, HostedWorkerError> {
        let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WorkerCommand::Execute(Box::new(move |state| {
                let _ = reply_sender.send(operation(state));
            })))
            .map_err(|_| HostedWorkerError::Unavailable)?;
        reply_receiver
            .recv()
            .map_err(|_| HostedWorkerError::Terminated)
    }

    pub fn shutdown(mut self) -> Result<(), HostedWorkerError> {
        self.stop()
    }

    fn stop(&mut self) -> Result<(), HostedWorkerError> {
        let _ = self.sender.send(WorkerCommand::Shutdown);
        let Some(join) = self.join.take() else {
            return Ok(());
        };
        join.join().map_err(|_| HostedWorkerError::Panicked)
    }
}

impl<T: 'static> Drop for HostedSessionWorker<T> {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn run_worker<T: 'static>(init: impl FnOnce() -> T, receiver: Receiver<WorkerCommand<T>>) {
    let mut state = init();
    while let Ok(command) = receiver.recv() {
        match command {
            WorkerCommand::Execute(operation) => operation(&mut state),
            WorkerCommand::Shutdown => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;

    #[test]
    fn confines_non_send_state_to_its_worker() {
        let worker = HostedSessionWorker::spawn(|| Rc::new(RefCell::new(7_u32)));
        let result = worker
            .execute(|state| {
                *state.borrow_mut() += 5;
                *state.borrow()
            })
            .expect("worker must answer");
        assert_eq!(result, 12);
        worker.shutdown().expect("worker must stop");
    }

    #[test]
    fn reports_session_initialization_without_moving_non_send_state() {
        let worker =
            HostedSessionWorker::try_spawn(|| Ok::<_, &'static str>(Rc::new(RefCell::new(9_u32))))
                .expect("initialization must succeed");
        assert_eq!(
            worker
                .execute(|state| *state.borrow())
                .expect("worker must answer"),
            9
        );
        worker.shutdown().expect("worker must stop");
    }
}
