use astra_core::Hash256;
use sha2::{Digest, Sha256};
use std::{
    fmt::Write as FmtWrite,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};
use thiserror::Error;

pub const DEFAULT_PERFETTO_MAX_BYTES: u64 = 512 * 1024 * 1024;
pub const DEFAULT_PERFETTO_MAX_EVENTS: u64 = 2_000_000;

#[derive(Debug, Error)]
pub enum PerfettoTraceError {
    #[error("ASTRA_PERFETTO_TRACE_CONFIG: {0}")]
    InvalidConfig(String),
    #[error("ASTRA_PERFETTO_TRACE_EVENT: {0}")]
    InvalidEvent(String),
    #[error("ASTRA_PERFETTO_TRACE_LIMIT: {0}")]
    Limit(String),
    #[error("ASTRA_PERFETTO_TRACE_IO: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct PerfettoTraceConfig {
    pub output_path: PathBuf,
    pub process_name: String,
    pub process_id: u32,
    pub max_bytes: u64,
    pub max_events: u64,
}

impl PerfettoTraceConfig {
    pub fn production(output_path: impl Into<PathBuf>, process_name: impl Into<String>) -> Self {
        Self {
            output_path: output_path.into(),
            process_name: process_name.into(),
            process_id: std::process::id(),
            max_bytes: DEFAULT_PERFETTO_MAX_BYTES,
            max_events: DEFAULT_PERFETTO_MAX_EVENTS,
        }
    }

    fn validate(&self) -> Result<(), PerfettoTraceError> {
        if self.output_path.file_name().is_none()
            || !safe_name(&self.process_name)
            || self.max_bytes < 4096
            || self.max_bytes > DEFAULT_PERFETTO_MAX_BYTES
            || self.max_events == 0
            || self.max_events > DEFAULT_PERFETTO_MAX_EVENTS
        {
            return Err(PerfettoTraceError::InvalidConfig(
                "path, process identity, or bounds are invalid".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfettoFlowPhase {
    Start,
    Step,
    End,
}

impl PerfettoFlowPhase {
    fn phase(self) -> &'static str {
        match self {
            Self::Start => "s",
            Self::Step => "t",
            Self::End => "f",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerfettoTraceSummary {
    pub trace_hash: Hash256,
    pub byte_length: u64,
    pub event_count: u64,
    pub dropped_event_count: u64,
    pub truncated: bool,
    pub timestamps_monotonic: bool,
}

pub struct PerfettoTraceWriter {
    config: PerfettoTraceConfig,
    temporary_path: PathBuf,
    writer: Option<BufWriter<File>>,
    event_count: u64,
    bytes_written: u64,
    last_timestamp_ns: u64,
    first_event: bool,
    finished: bool,
}

struct TraceArgs {
    frame: Option<u64>,
    value: Option<u64>,
    duration_ns: Option<u64>,
}

struct FixedEventBuffer {
    bytes: [u8; 1024],
    len: usize,
}

impl FixedEventBuffer {
    fn new() -> Self {
        Self {
            bytes: [0; 1024],
            len: 0,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl std::fmt::Write for FixedEventBuffer {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        let end = self.len.checked_add(value.len()).ok_or(std::fmt::Error)?;
        let destination = self.bytes.get_mut(self.len..end).ok_or(std::fmt::Error)?;
        destination.copy_from_slice(value.as_bytes());
        self.len = end;
        Ok(())
    }
}

impl PerfettoTraceWriter {
    pub fn create(config: PerfettoTraceConfig) -> Result<Self, PerfettoTraceError> {
        config.validate()?;
        if let Some(parent) = config.output_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary_path = temporary_path(&config.output_path)?;
        let file = File::create(&temporary_path)?;
        let mut writer = BufWriter::with_capacity(64 * 1024, file);
        let prefix = format!(
            "{{\"displayTimeUnit\":\"ns\",\"otherData\":{{\"process_name\":{}}},\"traceEvents\":[",
            serde_json::to_string(&config.process_name)
                .map_err(|error| PerfettoTraceError::InvalidConfig(error.to_string()))?
        );
        writer.write_all(prefix.as_bytes())?;
        Ok(Self {
            config,
            temporary_path,
            writer: Some(writer),
            event_count: 0,
            bytes_written: prefix.len() as u64,
            last_timestamp_ns: 0,
            first_event: true,
            finished: false,
        })
    }

    pub fn complete(
        &mut self,
        domain: &str,
        name: &str,
        thread_id: u32,
        frame: Option<u64>,
        start_ns: u64,
        duration_ns: u64,
    ) -> Result<(), PerfettoTraceError> {
        self.event(
            domain,
            name,
            thread_id,
            "X",
            start_ns,
            Some(duration_ns),
            None,
            TraceArgs {
                frame,
                value: None,
                duration_ns: Some(duration_ns),
            },
        )
    }

    pub fn begin(
        &mut self,
        domain: &str,
        name: &str,
        thread_id: u32,
        frame: Option<u64>,
        timestamp_ns: u64,
    ) -> Result<(), PerfettoTraceError> {
        self.event(
            domain,
            name,
            thread_id,
            "B",
            timestamp_ns,
            None,
            None,
            TraceArgs {
                frame,
                value: None,
                duration_ns: None,
            },
        )
    }

    pub fn end(
        &mut self,
        domain: &str,
        name: &str,
        thread_id: u32,
        frame: Option<u64>,
        timestamp_ns: u64,
    ) -> Result<(), PerfettoTraceError> {
        self.event(
            domain,
            name,
            thread_id,
            "E",
            timestamp_ns,
            None,
            None,
            TraceArgs {
                frame,
                value: None,
                duration_ns: None,
            },
        )
    }

    pub fn counter(
        &mut self,
        domain: &str,
        name: &str,
        timestamp_ns: u64,
        value: u64,
    ) -> Result<(), PerfettoTraceError> {
        self.event(
            domain,
            name,
            0,
            "C",
            timestamp_ns,
            None,
            None,
            TraceArgs {
                frame: None,
                value: Some(value),
                duration_ns: None,
            },
        )
    }

    pub fn flow(
        &mut self,
        domain: &str,
        name: &str,
        thread_id: u32,
        flow_id: u64,
        timestamp_ns: u64,
        phase: PerfettoFlowPhase,
    ) -> Result<(), PerfettoTraceError> {
        self.event(
            domain,
            name,
            thread_id,
            phase.phase(),
            timestamp_ns,
            None,
            Some(flow_id),
            TraceArgs {
                frame: None,
                value: None,
                duration_ns: None,
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn event(
        &mut self,
        domain: &str,
        name: &str,
        thread_id: u32,
        phase: &str,
        timestamp_ns: u64,
        duration_ns: Option<u64>,
        flow_id: Option<u64>,
        args: TraceArgs,
    ) -> Result<(), PerfettoTraceError> {
        if self.finished || !safe_name(domain) || !safe_name(name) {
            return Err(PerfettoTraceError::InvalidEvent(
                "writer state, domain, or event name is invalid".into(),
            ));
        }
        if self.event_count != 0 && timestamp_ns < self.last_timestamp_ns {
            return Err(PerfettoTraceError::InvalidEvent(
                "event timestamp moved backwards".into(),
            ));
        }
        if self.event_count == self.config.max_events {
            return Err(PerfettoTraceError::Limit(
                "event count reached the configured bound".into(),
            ));
        }
        let mut encoded = FixedEventBuffer::new();
        write!(
            encoded,
            "{{\"name\":\"{name}\",\"cat\":\"{domain}\",\"ph\":\"{phase}\",\"ts\":{},\"pid\":{},\"tid\":{}",
            timestamp_ns / 1_000,
            self.config.process_id,
            thread_id
        )
        .map_err(|_| PerfettoTraceError::Limit("encoded event exceeds fixed bound".into()))?;
        if let Some(duration_ns) = duration_ns {
            write!(encoded, ",\"dur\":{}", duration_ns.div_ceil(1_000)).map_err(|_| {
                PerfettoTraceError::Limit("encoded event exceeds fixed bound".into())
            })?;
        }
        if let Some(flow_id) = flow_id {
            write!(encoded, ",\"id\":{flow_id}").map_err(|_| {
                PerfettoTraceError::Limit("encoded event exceeds fixed bound".into())
            })?;
        }
        encoded
            .write_str(",\"args\":{")
            .map_err(|_| PerfettoTraceError::Limit("encoded event exceeds fixed bound".into()))?;
        let mut has_arg = false;
        for (key, value) in [
            ("frame", args.frame),
            ("value", args.value),
            ("duration_ns", args.duration_ns),
        ] {
            if let Some(value) = value {
                if has_arg {
                    encoded.write_char(',').map_err(|_| {
                        PerfettoTraceError::Limit("encoded event exceeds fixed bound".into())
                    })?;
                }
                write!(encoded, "\"{key}\":{value}").map_err(|_| {
                    PerfettoTraceError::Limit("encoded event exceeds fixed bound".into())
                })?;
                has_arg = true;
            }
        }
        encoded
            .write_str("}}")
            .map_err(|_| PerfettoTraceError::Limit("encoded event exceeds fixed bound".into()))?;
        let delimiter = if self.first_event { 0 } else { 1 };
        let projected = self
            .bytes_written
            .checked_add(delimiter + encoded.len as u64 + 2)
            .ok_or_else(|| PerfettoTraceError::Limit("trace byte count overflowed".into()))?;
        if projected > self.config.max_bytes {
            return Err(PerfettoTraceError::Limit(
                "trace reached the configured byte bound".into(),
            ));
        }
        let writer = self.writer.as_mut().ok_or_else(|| {
            PerfettoTraceError::InvalidEvent("trace writer is already closed".into())
        })?;
        if !self.first_event {
            writer.write_all(b",")?;
            self.bytes_written += 1;
        }
        writer.write_all(encoded.as_bytes())?;
        self.bytes_written += encoded.len as u64;
        self.event_count += 1;
        self.last_timestamp_ns = timestamp_ns;
        self.first_event = false;
        Ok(())
    }

    pub fn finish(mut self) -> Result<PerfettoTraceSummary, PerfettoTraceError> {
        if self.event_count == 0 {
            return Err(PerfettoTraceError::InvalidEvent(
                "trace contains no events".into(),
            ));
        }
        let mut writer = self.writer.take().ok_or_else(|| {
            PerfettoTraceError::InvalidEvent("trace writer is already closed".into())
        })?;
        writer.write_all(b"]}")?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        drop(writer);
        self.bytes_written += 2;
        fs::rename(&self.temporary_path, &self.config.output_path)?;
        self.finished = true;
        let (trace_hash, byte_length) = hash_file(&self.config.output_path)?;
        if byte_length != self.bytes_written {
            return Err(PerfettoTraceError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "trace byte length changed during finalization",
            )));
        }
        Ok(PerfettoTraceSummary {
            trace_hash,
            byte_length,
            event_count: self.event_count,
            dropped_event_count: 0,
            truncated: false,
            timestamps_monotonic: true,
        })
    }
}

impl Drop for PerfettoTraceWriter {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.writer.take();
            let _ = fs::remove_file(&self.temporary_path);
        }
    }
}

fn temporary_path(output: &Path) -> Result<PathBuf, PerfettoTraceError> {
    let name = output
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| PerfettoTraceError::InvalidConfig("output filename is invalid".into()))?;
    Ok(output.with_file_name(format!(".{name}.partial-{}", std::process::id())))
}

fn hash_file(path: &Path) -> Result<(Hash256, u64), PerfettoTraceError> {
    let file = File::open(path)?;
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut hasher = Sha256::new();
    let mut length = 0u64;
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        length = length
            .checked_add(read as u64)
            .ok_or_else(|| PerfettoTraceError::Limit("trace length overflowed".into()))?;
    }
    Ok((Hash256::from_bytes(hasher.finalize().into()), length))
}

fn safe_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[astra_headless_test::test]
    fn streams_importable_trace_without_retaining_events() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("trace.json");
        let mut writer = PerfettoTraceWriter::create(PerfettoTraceConfig {
            output_path: output.clone(),
            process_name: "astra-headless".into(),
            process_id: 7,
            max_bytes: 64 * 1024,
            max_events: 16,
        })
        .unwrap();
        writer
            .complete("renderer", "scene.submit", 1, Some(3), 1_000, 4_000)
            .unwrap();
        writer
            .begin("runtime", "input.consume", 1, Some(3), 5_000)
            .unwrap();
        writer
            .end("runtime", "input.consume", 1, Some(3), 5_000)
            .unwrap();
        writer
            .counter("memory", "working_set.bytes", 5_000, 1024)
            .unwrap();
        writer
            .flow(
                "input",
                "physical_input",
                1,
                9,
                6_000,
                PerfettoFlowPhase::Start,
            )
            .unwrap();
        let summary = writer.finish().unwrap();
        assert_eq!(summary.event_count, 5);
        assert_eq!(summary.dropped_event_count, 0);
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
        assert_eq!(value["traceEvents"].as_array().unwrap().len(), 5);
        if let Some(destination) = std::env::var_os("ASTRA_PERFETTO_SMOKE_OUTPUT") {
            std::fs::copy(output, destination).unwrap();
        }
    }

    #[astra_headless_test::test]
    fn rejects_timestamp_regression_and_removes_partial_trace() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("trace.json");
        let mut writer = PerfettoTraceWriter::create(PerfettoTraceConfig {
            output_path: output.clone(),
            process_name: "astra-headless".into(),
            process_id: 7,
            max_bytes: 4096,
            max_events: 2,
        })
        .unwrap();
        writer
            .counter("memory", "working_set.bytes", 2_000, 1)
            .unwrap();
        assert!(writer.counter("memory", "private.bytes", 1_000, 1).is_err());
        drop(writer);
        assert!(!output.exists());
    }
}
