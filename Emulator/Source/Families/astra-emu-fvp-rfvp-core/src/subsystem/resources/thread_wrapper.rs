#[cfg(feature = "no_std")]
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use std::collections::VecDeque;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ThreadRequest {
    /// start a new thread with the given id and address
    Start(u32, u32),
    /// wait for the given time
    Wait(u32),
    /// wait until dissolve is completed / static
    DissolveWait(),
    /// sleep for the given time, depercated request
    Sleep(u32),
    /// raise the threads which are waiting for the given time, depercated request
    Raise(u32),
    /// yield the current thread
    Next(),
    /// wait for text reveal to complete on the given thread
    TextWait(u32),
    /// resume a thread blocked by text reveal
    TextResume(u32),
    /// exit the corresponding thread, None is for all threads,
    /// If all threads are exited, the game will be impossible to manipulate through the script engine
    Exit(Option<u32>),
    ShouldBreak(),
}

#[derive(Default)]
pub struct ThreadWrapper {
    requests: VecDeque<ThreadRequest>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThreadWrapperSnapshotV1 {
    requests: VecDeque<ThreadRequest>,
}

impl ThreadWrapper {
    pub fn capture_snapshot_v1(&self) -> ThreadWrapperSnapshotV1 {
        ThreadWrapperSnapshotV1 {
            requests: self.requests.clone(),
        }
    }

    pub fn apply_snapshot_v1(&mut self, snapshot: ThreadWrapperSnapshotV1) {
        self.requests = snapshot.requests;
    }

    pub fn new() -> Self {
        Default::default()
    }

    pub fn pop(&mut self) -> Option<ThreadRequest> {
        self.requests.pop_front()
    }

    pub fn thread_start(&mut self, id: u32, addr: u32) {
        self.requests.push_back(ThreadRequest::Start(id, addr));
    }

    pub fn thread_wait(&mut self, time: u32) {
        self.requests.push_back(ThreadRequest::Wait(time));
    }

    pub fn dissolve_wait(&mut self) {
        self.requests.push_back(ThreadRequest::DissolveWait());
    }

    pub fn thread_sleep(&mut self, time: u32) {
        self.requests.push_back(ThreadRequest::Sleep(time));
    }

    pub fn thread_raise(&mut self, time: u32) {
        self.requests.push_back(ThreadRequest::Raise(time));
    }

    pub fn thread_next(&mut self) {
        self.requests.push_back(ThreadRequest::Next());
    }

    pub fn thread_text_wait(&mut self, id: u32) {
        self.requests.push_back(ThreadRequest::TextWait(id));
    }

    pub fn thread_text_resume(&mut self, id: u32) {
        self.requests.push_back(ThreadRequest::TextResume(id));
    }

    pub fn thread_exit(&mut self, id: Option<u32>) {
        self.requests.push_back(ThreadRequest::Exit(id));
    }

    pub fn should_break(&mut self) {
        self.requests.push_back(ThreadRequest::ShouldBreak());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_preserves_deferred_requests_for_the_next_vm_tick() {
        let mut original = ThreadWrapper::new();
        original.thread_start(7, 42);
        original.thread_wait(16);
        original.thread_text_resume(7);

        let mut restored = ThreadWrapper::new();
        restored.apply_snapshot_v1(original.capture_snapshot_v1());

        assert!(matches!(restored.pop(), Some(ThreadRequest::Start(7, 42))));
        assert!(matches!(restored.pop(), Some(ThreadRequest::Wait(16))));
        assert!(matches!(restored.pop(), Some(ThreadRequest::TextResume(7))));
        assert!(restored.pop().is_none());
    }
}
