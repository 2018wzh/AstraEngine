use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{ChildStdin, ChildStdout},
    sync::mpsc::{self, Receiver, SyncSender},
    thread::JoinHandle,
};

use astra_vn_editor::{PreviewRequest, PreviewResponse, PREVIEW_MAX_MESSAGE_BYTES};

/// The child must exit before join, so blocked pipe reads/writes are released.
pub(crate) struct PreviewPipe {
    requests: Option<SyncSender<Vec<u8>>>,
    pub responses: Receiver<anyhow::Result<PreviewResponse>>,
    reader: Option<JoinHandle<()>>,
    writer: Option<JoinHandle<()>>,
}

fn read_response(reader: &mut impl BufRead) -> anyhow::Result<Option<PreviewResponse>> {
    let mut line = Vec::new();
    let bytes = reader
        .take(PREVIEW_MAX_MESSAGE_BYTES as u64 + 1)
        .read_until(b'\n', &mut line)?;
    if bytes == 0 {
        return Ok(None);
    }
    anyhow::ensure!(
        bytes <= PREVIEW_MAX_MESSAGE_BYTES && line.last() == Some(&b'\n'),
        "Player control response is oversized or incomplete"
    );
    Ok(Some(serde_json::from_slice(&line)?))
}

impl PreviewPipe {
    pub fn new(mut input: ChildStdin, output: ChildStdout) -> Self {
        let (requests, receive) = mpsc::sync_channel::<Vec<u8>>(4);
        let (send, responses) = mpsc::sync_channel(4);
        let errors = send.clone();
        let writer = std::thread::spawn(move || {
            while let Ok(bytes) = receive.recv() {
                if let Err(error) = input.write_all(&bytes).and_then(|()| input.flush()) {
                    let _ = errors.try_send(Err(error.into()));
                    break;
                }
            }
        });
        let reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                match read_response(&mut reader) {
                    Ok(Some(response)) => {
                        if send.try_send(Ok(response)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => {
                        let _ = send.try_send(Err(anyhow::anyhow!("Player control pipe closed")));
                        break;
                    }
                    Err(error) => {
                        let _ = send.try_send(Err(error));
                        break;
                    }
                }
            }
        });
        Self {
            requests: Some(requests),
            responses,
            reader: Some(reader),
            writer: Some(writer),
        }
    }

    pub fn send(&self, request: &PreviewRequest) -> anyhow::Result<()> {
        let mut bytes = serde_json::to_vec(request)?;
        bytes.push(b'\n');
        anyhow::ensure!(
            bytes.len() <= PREVIEW_MAX_MESSAGE_BYTES,
            "Preview request exceeds protocol limit"
        );
        self.requests.as_ref().unwrap().try_send(bytes)?;
        Ok(())
    }

    pub fn join(&mut self) {
        self.requests.take();
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }

    pub fn reader_finished(&self) -> bool {
        self.reader
            .as_ref()
            .is_some_and(|reader| reader.is_finished())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_responses_require_complete_json_lines() {
        assert!(matches!(
            read_response(&mut &b"{\"kind\":\"stopped\"}\n"[..]).unwrap(),
            Some(PreviewResponse::Stopped)
        ));
        assert!(read_response(&mut &b"{\"kind\":\"stopped\"}"[..]).is_err());
        assert!(read_response(&mut vec![b' '; PREVIEW_MAX_MESSAGE_BYTES + 1].as_slice()).is_err());
        assert!(read_response(&mut &b""[..]).unwrap().is_none());
    }
}
