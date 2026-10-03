//! Native stdio is diagnostic-only. Always drain both pipes; keep line and
//! queue storage bounded and publish through the existing Family host sink.
use super::shared::Shared;
use astra_emu_family_api::{
    DiagnosticEvent, DiagnosticField, DiagnosticLevel, DiagnosticSinkBox, DiagnosticValue,
};
use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use std::{
    io::{self, Read},
    process::Child,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const TEXT_BYTES: usize = 4096;
const QUEUE_RECORDS: usize = 64;
const POLL: Duration = Duration::from_millis(10);

#[derive(Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}
impl Stream {
    fn target(self) -> &'static str {
        match self {
            Self::Stdout => "krkr.native.stdout",
            Self::Stderr => "krkr.native.stderr",
        }
    }
    fn level(self) -> DiagnosticLevel {
        match self {
            Self::Stdout => DiagnosticLevel::Info,
            Self::Stderr => DiagnosticLevel::Warn,
        }
    }
}
struct Record {
    stream: Stream,
    text: String,
    truncated_bytes: u64,
}

pub(super) struct Drain {
    stop: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    sender: Option<Sender<Record>>,
    readers: Vec<JoinHandle<io::Result<()>>>,
    publisher: Option<JoinHandle<()>>,
}

impl Drain {
    pub fn start(
        child: &mut Child,
        sink: Arc<DiagnosticSinkBox>,
        shared: Arc<Shared>,
    ) -> io::Result<Self> {
        let (sender, receiver) = bounded(QUEUE_RECORDS);
        let mut drain = Self {
            stop: Arc::new(AtomicBool::new(false)),
            dropped: Arc::new(AtomicU64::new(0)),
            sender: Some(sender),
            readers: Vec::new(),
            publisher: None,
        };
        let dropped = Arc::clone(&drain.dropped);
        let publisher_shared = Arc::clone(&shared);
        drain.publisher = Some(
            thread::Builder::new()
                .name("krkr-diagnostics-publish".to_owned())
                .spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        publish(receiver, sink, dropped)
                    }));
                    if result.is_err() {
                        publisher_shared.fail(crate::wire::Failure::Protocol);
                    }
                })?,
        );
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing native stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("missing native stderr"))?;
        drain.reader(stdout, Stream::Stdout, Arc::clone(&shared))?;
        drain.reader(stderr, Stream::Stderr, shared)?;
        Ok(drain)
    }

    fn reader(
        &mut self,
        mut reader: impl NativeReader,
        stream: Stream,
        shared: Arc<Shared>,
    ) -> io::Result<()> {
        nonblocking(&reader)?;
        let sender = self.sender.as_ref().unwrap().clone();
        let stop = Arc::clone(&self.stop);
        let dropped = Arc::clone(&self.dropped);
        let worker = thread::Builder::new()
            .name(stream.target().to_owned())
            .spawn(move || {
                let result = drain_lines(&mut reader, stream, &sender, &stop, &dropped);
                if result.is_err() && !stop.load(Ordering::Acquire) {
                    shared.fail(crate::wire::Failure::Protocol);
                }
                result
            })?;
        self.readers.push(worker);
        Ok(())
    }

    pub fn finish(&mut self) -> io::Result<()> {
        let eof_deadline = Instant::now() + Duration::from_millis(100);
        while self.readers.iter().any(|worker| !worker.is_finished())
            && Instant::now() < eof_deadline
        {
            thread::sleep(POLL);
        }
        self.stop.store(true, Ordering::Release);
        let mut failed = false;
        for worker in self.readers.drain(..) {
            // A child or descendant may have inherited a diagnostic pipe.
            // Cancel reads directly rather than waiting indefinitely for EOF.
            #[cfg(windows)]
            while !worker.is_finished() {
                use std::os::windows::io::AsRawHandle;
                unsafe {
                    windows_sys::Win32::System::IO::CancelSynchronousIo(worker.as_raw_handle());
                }
                thread::sleep(POLL);
            }
            if worker.join().is_err() {
                failed = true;
            }
        }
        self.sender.take();
        if let Some(publisher) = self.publisher.take() {
            if publisher.join().is_err() {
                failed = true;
            }
        }
        if failed {
            Err(io::Error::other("native diagnostic worker panicked"))
        } else {
            Ok(())
        }
    }
}
impl Drop for Drain {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[cfg(unix)]
trait NativeReader: Read + Send + std::os::fd::AsRawFd + 'static {}
#[cfg(unix)]
impl<T: Read + Send + std::os::fd::AsRawFd + 'static> NativeReader for T {}
#[cfg(windows)]
trait NativeReader: Read + Send + 'static {}
#[cfg(windows)]
impl<T: Read + Send + 'static> NativeReader for T {}

#[cfg(unix)]
fn nonblocking(reader: &impl NativeReader) -> io::Result<()> {
    let fd = reader.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
#[cfg(windows)]
fn nonblocking(_: &impl NativeReader) -> io::Result<()> {
    Ok(())
}

fn send_line(
    sender: &Sender<Record>,
    stream: Stream,
    line: &[u8],
    truncated_bytes: u64,
    dropped: &AtomicU64,
) {
    let mut text = String::from_utf8_lossy(line).into_owned();
    let mut end = text.len().min(TEXT_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let truncated_bytes = truncated_bytes.saturating_add((text.len() - end) as u64);
    text.truncate(end);
    if let Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) = sender.try_send(Record {
        stream,
        text,
        truncated_bytes,
    }) {
        dropped.fetch_add(1, Ordering::Relaxed);
    }
}

fn drain_lines(
    reader: &mut impl Read,
    stream: Stream,
    sender: &Sender<Record>,
    stop: &AtomicBool,
    dropped: &AtomicU64,
) -> io::Result<()> {
    let mut buffer = [0u8; TEXT_BYTES];
    let mut line = Vec::with_capacity(TEXT_BYTES);
    let mut truncated = 0u64;
    while !stop.load(Ordering::Acquire) {
        let count = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(POLL);
                continue;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) if stop.load(Ordering::Acquire) => break,
            Err(error) => return Err(error),
        };
        for byte in &buffer[..count] {
            if *byte == b'\n' {
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                send_line(sender, stream, &line, truncated, dropped);
                line.clear();
                truncated = 0;
            } else if line.len() < TEXT_BYTES {
                line.push(*byte);
            } else {
                truncated = truncated.saturating_add(1);
            }
        }
    }
    if !line.is_empty() || truncated > 0 {
        send_line(sender, stream, &line, truncated, dropped);
    }
    Ok(())
}

fn publish(receiver: Receiver<Record>, sink: Arc<DiagnosticSinkBox>, dropped: Arc<AtomicU64>) {
    while let Ok(record) = receiver.recv() {
        let level = record.stream.level();
        if sink.enabled(level) {
            sink.emit(DiagnosticEvent {
                level,
                target: record.stream.target().into(),
                event: "krkr.native.output".into(),
                fields: vec![
                    DiagnosticField {
                        name: "message".into(),
                        value: DiagnosticValue::Text(record.text.into()),
                    },
                    DiagnosticField {
                        name: "truncated_bytes".into(),
                        value: DiagnosticValue::Unsigned(record.truncated_bytes),
                    },
                ]
                .into(),
                dropped_fields: 0,
            });
        }
        publish_dropped(&sink, &dropped);
    }
    publish_dropped(&sink, &dropped);
}
fn publish_dropped(sink: &DiagnosticSinkBox, dropped: &AtomicU64) {
    let count = dropped.swap(0, Ordering::AcqRel);
    if count > 0 && sink.enabled(DiagnosticLevel::Warn) {
        sink.emit(DiagnosticEvent {
            level: DiagnosticLevel::Warn,
            target: "krkr.native".into(),
            event: "krkr.native.output_dropped".into(),
            fields: vec![DiagnosticField {
                name: "records".into(),
                value: DiagnosticValue::Unsigned(count),
            }]
            .into(),
            dropped_fields: 0,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unterminated_lines_invalid_utf8_and_full_queue_remain_bounded() {
        let (sender, receiver) = bounded(1);
        let input = vec![0xff; TEXT_BYTES * 32];
        let stop = AtomicBool::new(false);
        let dropped = AtomicU64::new(0);
        drain_lines(
            &mut input.as_slice(),
            Stream::Stderr,
            &sender,
            &stop,
            &dropped,
        )
        .unwrap();
        let record = receiver.recv().unwrap();
        assert!(record.text.len() <= TEXT_BYTES);
        assert!(record.truncated_bytes > 0);
        send_line(&sender, Stream::Stdout, b"first", 0, &dropped);
        send_line(&sender, Stream::Stdout, b"second", 0, &dropped);
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
        assert_eq!(receiver.len(), 1);
    }
}
