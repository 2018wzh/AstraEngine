use astra_emu_abi as abi;
use parking_lot::Mutex;
use std::{
    ffi::c_void,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
struct State {
    epoch: u64,
    cancelled: AtomicBool,
    publication: Mutex<()>,
}
/// Exists before native open starts; retained through failed open and close.
#[derive(Clone)]
pub struct NativeCancellation(Arc<State>);
impl NativeCancellation {
    pub fn new(epoch: u64) -> Self {
        Self(Arc::new(State {
            epoch,
            cancelled: AtomicBool::new(false),
            publication: Mutex::new(()),
        }))
    }
    pub fn cancel(&self) {
        let _guard = self.0.publication.lock();
        self.0.cancelled.store(true, Ordering::Release);
    }
    pub(crate) fn publish(&self) -> bool {
        let _guard = self.0.publication.lock();
        !self.is_cancelled()
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }
    pub fn epoch(&self) -> u64 {
        self.0.epoch
    }
    pub(crate) fn service(&self) -> abi::CancellationService {
        abi::CancellationService {
            context: Arc::as_ptr(&self.0).cast_mut().cast(),
            is_cancelled: Some(is_cancelled),
        }
    }
}
unsafe extern "C" fn is_cancelled(context: *mut c_void, epoch: u64) -> u32 {
    let Some(state) = (unsafe { context.cast::<State>().as_ref() }) else {
        return 1;
    };
    u32::from(state.epoch != epoch || state.cancelled.load(Ordering::Acquire))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_before_publication_rejects_handle() {
        let token = NativeCancellation::new(1);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let worker_token = token.clone();
        let worker_barrier = barrier.clone();
        let worker = std::thread::spawn(move || {
            worker_token.cancel();
            worker_barrier.wait();
        });
        barrier.wait();
        assert!(!token.publish());
        let service = token.service();
        assert_eq!(
            unsafe { service.is_cancelled.unwrap()(service.context, 1) },
            1
        );
        worker.join().unwrap();
    }
}
