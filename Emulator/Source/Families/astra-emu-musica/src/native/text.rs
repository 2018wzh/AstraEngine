use crate::scene::error;
use astra_emu_abi as abi;
use astra_emu_family_api::FamilyResult;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

pub(crate) struct ServiceContext {
    pub session: u64,
    pub epoch: AtomicU64,
    pub cancelled: AtomicBool,
    pub waiting: AtomicBool,
    pub text: Option<abi::TextService>,
    pub window: Option<abi::WindowService>,
    pub host_cancel: Option<abi::CancellationService>,
}
// Host service contexts explicitly support the native worker and independent
// concurrent invalidation. Their storage remains alive until close joins it.
unsafe impl Send for ServiceContext {}
unsafe impl Sync for ServiceContext {}
impl ServiceContext {
    pub fn is_cancelled(&self) -> bool {
        if let Some(service) = self.host_cancel {
            if service
                .is_cancelled
                .is_some_and(|check| unsafe { check(service.context, self.session) } != 0)
                && !self.cancelled.swap(true, Ordering::AcqRel)
            {
                self.invalidate();
            }
        }
        self.cancelled.load(Ordering::Acquire)
    }
    pub fn invalidate(&self) {
        let next = match self
            .epoch
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |epoch| {
                epoch.checked_add(1)
            }) {
            Ok(previous) => previous + 1,
            Err(epoch) => {
                self.cancelled.store(true, Ordering::Release);
                epoch
            }
        };
        if let Some(service) = self.text {
            if let Some(invalidate) = service.invalidate {
                unsafe { invalidate(service.context, self.session, next) };
            }
        }
        if let Some(service) = self.window {
            if let Some(invalidate) = service.invalidate {
                unsafe { invalidate(service.context, self.session, next) };
            }
        }
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.invalidate();
    }
}
#[derive(Clone)]
pub(crate) struct TextBridge(pub Arc<ServiceContext>);
impl TextBridge {
    pub fn invalidate(&self) {
        self.0.invalidate();
    }
    pub fn check_cancelled(&self) -> FamilyResult<()> {
        if self.0.is_cancelled() {
            Err(error(
                "ASTRA_EMU_MUSICA_CANCELLED",
                "native session was cancelled",
            ))
        } else {
            Ok(())
        }
    }
    pub fn transform(&self, id: u64, source: &[(u32, &str)]) -> FamilyResult<Vec<String>> {
        if source.len() > 128
            || source
                .iter()
                .any(|(_, text)| text.len() > astra_emu_family_api::MAX_TEXT_BYTES)
            || source.iter().map(|(_, text)| text.len()).sum::<usize>() > 1024 * 1024
        {
            return Err(error(
                "ASTRA_EMU_MUSICA_TEXT_BOUND",
                "semantic text request exceeds its bounds",
            ));
        }
        let service = self
            .0
            .text
            .ok_or_else(|| error("ASTRA_EMU_MUSICA_TEXT_SERVICE", "text service is missing"))?;
        let epoch = self.0.epoch.load(Ordering::Acquire);
        if self.0.is_cancelled() {
            return Err(error(
                "ASTRA_EMU_MUSICA_TEXT_CANCELLED",
                "text context was cancelled",
            ));
        }
        let items: Vec<_> = source
            .iter()
            .enumerate()
            .map(|(index, (kind, text))| abi::TextItem {
                id: index as u64 + 1,
                kind: *kind,
                reserved: 0,
                text: abi::Bytes::new(text.as_bytes()),
            })
            .collect();
        let request = abi::TextRequest {
            session_epoch: self.0.session,
            context_epoch: epoch,
            request_id: id,
            items: items.as_ptr(),
            item_count: items.len(),
        };
        let mut bytes = vec![
            0;
            source
                .iter()
                .map(|(_, text)| text.len())
                .sum::<usize>()
                .max(1024)
        ];
        let mut ranges = vec![
            abi::TextRange {
                id: 0,
                offset: 0,
                len: 0
            };
            items.len()
        ];
        let request_fn = service
            .request
            .ok_or_else(|| error("ASTRA_EMU_MUSICA_TEXT_SERVICE", "text callback is missing"))?;
        self.0.waiting.store(true, Ordering::Release);
        let mut resized = false;
        let result = (|| loop {
            let mut result = abi::TextResult {
                bytes: bytes.as_mut_ptr(),
                byte_capacity: bytes.len(),
                byte_count: 0,
                items: ranges.as_mut_ptr(),
                item_capacity: ranges.len(),
                item_count: 0,
            };
            let mut diagnostic = abi::Error::default();
            let status =
                unsafe { request_fn(service.context, &request, &mut result, &mut diagnostic) };
            if self.0.is_cancelled() || self.0.epoch.load(Ordering::Acquire) != epoch {
                return Err(error(
                    "ASTRA_EMU_MUSICA_TEXT_CANCELLED",
                    "late text result rejected",
                ));
            }
            if status == abi::CAPACITY {
                if resized
                    || result.byte_count > 1024 * 1024
                    || result.item_count != source.len()
                    || (result.byte_count <= bytes.len() && result.item_count <= ranges.len())
                {
                    return Err(error(
                        "ASTRA_EMU_MUSICA_TEXT_BOUND",
                        "invalid text result capacity",
                    ));
                }
                bytes.resize(result.byte_count, 0);
                resized = true;
                continue;
            }
            if status != abi::OK {
                let code = std::str::from_utf8(
                    &diagnostic.code[..(diagnostic.code_len as usize).min(diagnostic.code.len())],
                )
                .unwrap_or("ASTRA_EMU_MUSICA_TEXT_FAILED");
                let message = std::str::from_utf8(
                    &diagnostic.message
                        [..(diagnostic.message_len as usize).min(diagnostic.message.len())],
                )
                .unwrap_or("text operation failed");
                return Err(error(
                    if code.is_empty() {
                        "ASTRA_EMU_MUSICA_TEXT_FAILED"
                    } else {
                        code
                    },
                    message,
                ));
            }
            if result.byte_count > bytes.len() || result.item_count != source.len() {
                return Err(error(
                    "ASTRA_EMU_MUSICA_TEXT_BOUND",
                    "invalid text result length",
                ));
            }
            let mut transformed = Vec::with_capacity(source.len());
            for (index, range) in ranges.iter().enumerate() {
                let end = range
                    .offset
                    .checked_add(range.len)
                    .filter(|end| *end <= result.byte_count)
                    .ok_or_else(|| error("ASTRA_EMU_MUSICA_TEXT_BOUND", "invalid text range"))?;
                let limit = if source[index].0 == abi::TEXT_SPEAKER {
                    astra_emu_family_api::MAX_SYMBOL_BYTES
                } else {
                    astra_emu_family_api::MAX_TEXT_BYTES
                };
                if range.id != index as u64 + 1 || range.len > limit {
                    return Err(error(
                        "ASTRA_EMU_MUSICA_TEXT_ID",
                        "text result identity or length differs",
                    ));
                }
                let text = std::str::from_utf8(&bytes[range.offset..end])
                    .map_err(|_| error("ASTRA_EMU_MUSICA_TEXT_UTF8", "text result is not UTF-8"))?;
                if text.contains('\0') {
                    return Err(error(
                        "ASTRA_EMU_MUSICA_TEXT_UTF8",
                        "text result contains NUL",
                    ));
                }
                transformed.push(text.to_owned());
            }
            let acknowledge = service.acknowledge.ok_or_else(|| {
                error(
                    "ASTRA_EMU_MUSICA_TEXT_SERVICE",
                    "acknowledge callback is missing",
                )
            })?;
            if unsafe { acknowledge(service.context, self.0.session, epoch, id) } != abi::OK {
                return Err(error(
                    "ASTRA_EMU_MUSICA_TEXT_ACK",
                    "text result acknowledgement failed",
                ));
            }
            if self.0.is_cancelled() || self.0.epoch.load(Ordering::Acquire) != epoch {
                return Err(error(
                    "ASTRA_EMU_MUSICA_TEXT_CANCELLED",
                    "late text acknowledgement rejected",
                ));
            }
            return Ok(transformed);
        })();
        self.0.waiting.store(false, Ordering::Release);
        result
    }
}
