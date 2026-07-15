use std::io::{BufRead, Write};

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{operation}: {message}")]
pub struct ProtocolError {
    pub operation: &'static str,
    pub message: String,
}

impl ProtocolError {
    pub fn invalid(operation: &'static str, message: impl Into<String>) -> Self {
        Self {
            operation,
            message: message.into(),
        }
    }
}

pub struct JsonlReader<R> {
    reader: R,
    line: String,
    line_number: u64,
    max_line_bytes: usize,
}

impl<R: BufRead> JsonlReader<R> {
    pub fn new(reader: R, max_line_bytes: usize) -> Result<Self, ProtocolError> {
        if max_line_bytes == 0 {
            return Err(ProtocolError::invalid(
                "jsonl.open",
                "line limit must be non-zero",
            ));
        }
        Ok(Self {
            reader,
            line: String::new(),
            line_number: 0,
            max_line_bytes,
        })
    }

    pub fn read<T: DeserializeOwned>(&mut self) -> Result<Option<T>, ProtocolError> {
        self.line.clear();
        let read = self
            .reader
            .read_line(&mut self.line)
            .map_err(|e| ProtocolError::invalid("jsonl.read", e.to_string()))?;
        if read == 0 {
            return Ok(None);
        }
        self.line_number += 1;
        if read > self.max_line_bytes {
            return Err(ProtocolError::invalid(
                "jsonl.read",
                format!("line {} exceeds byte limit", self.line_number),
            ));
        }
        if !self.line.ends_with('\n') {
            return Err(ProtocolError::invalid(
                "jsonl.read",
                format!("line {} is not newline terminated", self.line_number),
            ));
        }
        let value =
            serde_json::from_str(self.line.trim_end_matches(['\r', '\n'])).map_err(|e| {
                ProtocolError::invalid("jsonl.decode", format!("line {}: {e}", self.line_number))
            })?;
        Ok(Some(value))
    }
}

pub struct JsonlWriter<W> {
    writer: W,
}

impl<W: Write> JsonlWriter<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    pub fn write<T: Serialize>(&mut self, value: &T) -> Result<(), ProtocolError> {
        serde_json::to_writer(&mut self.writer, value)
            .map_err(|e| ProtocolError::invalid("jsonl.encode", e.to_string()))?;
        self.writer
            .write_all(b"\n")
            .map_err(|e| ProtocolError::invalid("jsonl.write", e.to_string()))?;
        self.writer
            .flush()
            .map_err(|e| ProtocolError::invalid("jsonl.flush", e.to_string()))
    }
}

#[derive(Debug, Default, Clone)]
pub struct SequenceValidator {
    session: Option<String>,
    last_sequence: u64,
    last_tick: u64,
}

impl SequenceValidator {
    pub fn accept(&mut self, session: &str, sequence: u64, tick: u64) -> Result<(), ProtocolError> {
        if let Some(expected) = &self.session {
            if expected != session {
                return Err(ProtocolError::invalid(
                    "protocol.session",
                    "cross-session message",
                ));
            }
        } else {
            self.session = Some(session.to_owned());
        }
        if sequence == 0 || (self.last_sequence != 0 && sequence <= self.last_sequence) {
            return Err(ProtocolError::invalid(
                "protocol.sequence",
                "sequence must be strictly increasing",
            ));
        }
        if tick < self.last_tick {
            return Err(ProtocolError::invalid(
                "protocol.tick",
                "tick cannot move backwards",
            ));
        }
        self.last_sequence = sequence;
        self.last_tick = tick;
        Ok(())
    }
}
