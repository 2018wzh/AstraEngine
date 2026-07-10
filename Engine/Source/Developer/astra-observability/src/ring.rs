use std::collections::VecDeque;

#[derive(Debug)]
pub(crate) struct RingBuffer {
    records: VecDeque<String>,
    bytes: usize,
    max_records: usize,
    max_bytes: usize,
}

impl RingBuffer {
    pub(crate) fn new(max_records: usize, max_bytes: usize) -> Self {
        Self {
            records: VecDeque::new(),
            bytes: 0,
            max_records,
            max_bytes,
        }
    }

    pub(crate) fn push(&mut self, mut record: String) {
        if record.len() > self.max_bytes {
            record.truncate(self.max_bytes);
        }
        self.bytes += record.len();
        self.records.push_back(record);
        while self.records.len() > self.max_records || self.bytes > self.max_bytes {
            if let Some(record) = self.records.pop_front() {
                self.bytes = self.bytes.saturating_sub(record.len());
            } else {
                break;
            }
        }
    }

    pub(crate) fn snapshot(&self) -> Vec<String> {
        self.records.iter().cloned().collect()
    }
}
