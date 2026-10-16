use crate::{CompilationSnapshot, CompileAstraProjectOptions, VnError};
use parking_lot::{Condvar, Mutex};
use std::{
    io,
    sync::{mpsc, Arc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub type CompilationResult = Arc<Result<usize, VnError>>;

struct Request {
    deadline: Instant,
    snapshot: CompilationSnapshot,
    options: Arc<CompileAstraProjectOptions>,
    reply: mpsc::SyncSender<CompilationResult>,
}

#[derive(Default)]
struct State {
    pending: Option<Request>,
    closed: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    ready: Condvar,
}

/// One compiler owner and at most one pending request. Replacing a pending
/// request disconnects its receiver; an in-flight compile never blocks editing.
pub struct BackgroundCompiler {
    shared: Arc<Shared>,
    debounce: Duration,
    worker: Option<JoinHandle<()>>,
}

impl BackgroundCompiler {
    pub fn new(debounce: Duration) -> io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let owner = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("astra-editor-diagnostics".into())
            .spawn(move || run(owner))?;
        Ok(Self {
            shared,
            debounce,
            worker: Some(worker),
        })
    }

    pub fn submit(
        &self,
        snapshot: CompilationSnapshot,
        options: Arc<CompileAstraProjectOptions>,
    ) -> io::Result<mpsc::Receiver<CompilationResult>> {
        let deadline = Instant::now()
            .checked_add(self.debounce)
            .ok_or_else(|| io::Error::other("compiler debounce exceeds the clock range"))?;
        let (reply, receiver) = mpsc::sync_channel(1);
        let mut state = self.shared.state.lock();
        if state.closed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "compiler is closed",
            ));
        }
        state.pending = Some(Request {
            deadline,
            snapshot,
            options,
            reply,
        });
        self.shared.ready.notify_one();
        Ok(receiver)
    }

    pub fn close(&mut self) -> io::Result<()> {
        {
            let mut state = self.shared.state.lock();
            state.closed = true;
            state.pending = None;
            self.shared.ready.notify_one();
        }
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| io::Error::other("compiler worker panicked"))?;
        }
        Ok(())
    }
}

impl Drop for BackgroundCompiler {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            tracing::error!(event = "editor.compiler.shutdown_failed", %error);
        }
    }
}

struct CachedCompilation {
    snapshot: CompilationSnapshot,
    options: Arc<CompileAstraProjectOptions>,
    result: CompilationResult,
}

fn run(shared: Arc<Shared>) {
    let mut previous: Option<CachedCompilation> = None;
    loop {
        let request = {
            let mut state = shared.state.lock();
            loop {
                if state.closed {
                    return;
                }
                let Some(pending) = &state.pending else {
                    shared.ready.wait(&mut state);
                    continue;
                };
                let deadline = pending.deadline;
                if Instant::now() < deadline {
                    shared.ready.wait_until(&mut state, deadline);
                    continue;
                }
                break state
                    .pending
                    .take()
                    .expect("pending request checked while locked");
            }
        };
        let cached = previous.as_ref().filter(|cached| {
            cached.snapshot.same_revision(&request.snapshot)
                && Arc::ptr_eq(&cached.options, &request.options)
        });
        let result = match cached {
            Some(cached) => Arc::clone(&cached.result),
            None => Arc::new(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    request
                        .snapshot
                        .compile((*request.options).clone())
                        .map(|project| project.story.source_map.len())
                }))
                .unwrap_or_else(|_| Err(VnError::message("ASTRA_EDITOR_COMPILER_PANIC"))),
            ),
        };
        let closed = shared.state.lock().closed;
        if !closed {
            let _ = request.reply.send(Arc::clone(&result));
        }
        previous = Some(CachedCompilation {
            snapshot: request.snapshot,
            options: request.options,
            result,
        });
        if closed {
            return;
        }
    }
}
