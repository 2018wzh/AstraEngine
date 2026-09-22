//! Explicit parent/child pipe; no ports, background service, or persistent configuration.
use astra_vn_core::{
    PreviewRejectCode as Reject, PreviewRequest, PreviewResponse, PREVIEW_MAX_MESSAGE_BYTES,
};
use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub(crate) struct PreviewTransport {
    incoming: tokio::sync::mpsc::Receiver<Result<PreviewRequest, Reject>>,
    outgoing: Option<mpsc::SyncSender<Vec<u8>>>,
    stop: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
    #[cfg(unix)]
    original_flags: [i32; 2],
}
impl PreviewTransport {
    pub(crate) fn open() -> std::io::Result<Self> {
        #[cfg(unix)]
        let original_flags = make_nonblocking()?;
        let (input, incoming) = tokio::sync::mpsc::channel(4);
        let (outgoing, output) = mpsc::sync_channel::<Vec<u8>>(4);
        let stop = Arc::new(AtomicBool::new(false));
        let reader_stop = stop.clone();
        let reader = thread::spawn(move || {
            let mut stdin = pipe_file(true);
            let mut line = Vec::new();
            let mut buffer = [0u8; 4096];
            while !reader_stop.load(Ordering::Acquire) {
                match stdin.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        for byte in &buffer[..count] {
                            if *byte == b'\n' {
                                let parsed = serde_json::from_slice(&line)
                                    .map_err(|_| Reject::InvalidRequest);
                                line.clear();
                                let invalid = parsed.is_err();
                                if input.blocking_send(parsed).is_err() || invalid {
                                    return;
                                }
                            } else {
                                line.push(*byte);
                                if line.len() > PREVIEW_MAX_MESSAGE_BYTES {
                                    let _ = input.blocking_send(Err(Reject::InvalidRequest));
                                    return;
                                }
                            }
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        });
        let writer_stop = stop.clone();
        let writer = thread::spawn(move || {
            let mut stdout = pipe_file(false);
            while !writer_stop.load(Ordering::Acquire) {
                let bytes = match output.recv_timeout(Duration::from_millis(10)) {
                    Ok(bytes) => bytes,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => return,
                };
                let mut remaining = bytes.as_slice();
                while !remaining.is_empty() && !writer_stop.load(Ordering::Acquire) {
                    match stdout.write(remaining) {
                        Ok(0) => {
                            writer_stop.store(true, Ordering::Release);
                            return;
                        }
                        Ok(count) => remaining = &remaining[count..],
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5))
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => {
                            writer_stop.store(true, Ordering::Release);
                            return;
                        }
                    }
                }
                if stdout.flush().is_err() {
                    writer_stop.store(true, Ordering::Release);
                    return;
                }
            }
        });
        Ok(Self {
            incoming,
            outgoing: Some(outgoing),
            stop,
            workers: vec![reader, writer],
            #[cfg(unix)]
            original_flags,
        })
    }
    pub(crate) async fn recv(&mut self) -> Result<PreviewRequest, Reject> {
        loop {
            if self.stop.load(Ordering::Acquire) {
                return Err(Reject::Disconnected);
            }
            tokio::select! {
                value = self.incoming.recv() => return value.unwrap_or(Err(Reject::Disconnected)),
                _ = tokio::time::sleep(Duration::from_millis(20)) => {}
            }
        }
    }
    pub(crate) fn send(&self, response: &PreviewResponse) -> Result<(), Reject> {
        let mut bytes = serde_json::to_vec(response).map_err(|_| Reject::InvalidRequest)?;
        if bytes.len() > PREVIEW_MAX_MESSAGE_BYTES {
            return Err(Reject::InvalidRequest);
        }
        bytes.push(b'\n');
        self.outgoing
            .as_ref()
            .ok_or(Reject::Disconnected)?
            .try_send(bytes)
            .map_err(|_| Reject::Disconnected)
    }
}
impl Drop for PreviewTransport {
    fn drop(&mut self) {
        self.incoming.close();
        self.outgoing.take();
        // Let already queued replies drain, but never wait indefinitely for an editor
        // which stopped reading. Cancellation below interrupts both native pipe IOs.
        let deadline = std::time::Instant::now() + Duration::from_millis(100);
        while !self.workers[1].is_finished() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(2));
        }
        self.stop.store(true, Ordering::Release);
        for worker in self.workers.drain(..) {
            #[cfg(windows)]
            {
                use std::os::windows::io::AsRawHandle;
                while !worker.is_finished() {
                    // Repeated cancellation also covers cancellation racing the next ReadFile/WriteFile.
                    unsafe {
                        windows_sys::Win32::System::IO::CancelSynchronousIo(worker.as_raw_handle());
                    }
                    thread::sleep(Duration::from_millis(2));
                }
            }
            let _ = worker.join();
        }
        #[cfg(unix)]
        for (fd, flags) in [0, 1].into_iter().zip(self.original_flags) {
            unsafe {
                libc::fcntl(fd, libc::F_SETFL, flags);
            }
        }
    }
}
#[cfg(unix)]
fn make_nonblocking() -> std::io::Result<[i32; 2]> {
    let mut original = [0; 2];
    for fd in [0, 1] {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            if fd == 1 {
                unsafe {
                    libc::fcntl(0, libc::F_SETFL, original[0]);
                }
            }
            return Err(std::io::Error::last_os_error());
        }
        original[fd as usize] = flags;
    }
    Ok(original)
}

fn pipe_file(input: bool) -> std::mem::ManuallyDrop<std::fs::File> {
    #[cfg(windows)]
    {
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        let handle = if input {
            std::io::stdin().as_raw_handle()
        } else {
            std::io::stdout().as_raw_handle()
        };
        // Stdio owns this handle; the worker borrows it without buffered line IO.
        std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_handle(handle) })
    }
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd;
        std::mem::ManuallyDrop::new(unsafe {
            std::fs::File::from_raw_fd(if input { 0 } else { 1 })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "subprocess pipe lifecycle helper"]
    fn pipe_child() {
        let mode = std::env::var("ASTRA_PREVIEW_PIPE_TEST").unwrap();
        let mut transport = PreviewTransport::open().unwrap();
        if mode == "invalid" {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .unwrap();
            assert!(matches!(
                runtime.block_on(transport.recv()),
                Err(Reject::InvalidRequest)
            ));
        } else if mode == "blocked_output" {
            for _ in 0..4 {
                let _ = transport
                    .outgoing
                    .as_ref()
                    .unwrap()
                    .try_send(vec![b'x'; PREVIEW_MAX_MESSAGE_BYTES]);
            }
            thread::sleep(Duration::from_millis(20));
        } else {
            assert_eq!(mode, "1");
        }
        let _ = transport.send(&PreviewResponse::Stopped);
        drop(transport);
    }
    #[test]
    fn pipe_workers_join_with_stdin_still_open() {
        run_child("1", None);
    }
    #[test]
    fn malformed_and_oversize_messages_close_pipe() {
        run_child("invalid", Some(b"not-json\n"));
        run_child("invalid", Some(&vec![b'x'; PREVIEW_MAX_MESSAGE_BYTES + 1]));
    }
    #[test]
    fn blocked_stdout_is_cancelled_and_joined() {
        run_child("blocked_output", None);
    }
    fn run_child(mode: &str, input: Option<&[u8]>) {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "preview_transport::tests::pipe_child",
                "--ignored",
                "--nocapture",
            ])
            .env("ASTRA_PREVIEW_PIPE_TEST", mode)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(bytes) = input {
            child.stdin.as_mut().unwrap().write_all(bytes).unwrap();
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                let _ = child.wait();
                panic!("preview workers did not join");
            }
            thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        if mode != "blocked_output" {
            assert!(String::from_utf8_lossy(&output.stdout).contains("\"kind\":\"stopped\""));
        }
    }
}
