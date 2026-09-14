//! AstraEMU Artemis family plugin.
//!
//! Wraps the vendored Artemis runtime (`art3m1s-core`) behind the independent
//! Family ABI: CPU RGBA frames read back from the offscreen Vulkan target
//! after each engine tick, mixed PCM pushed from the adapter's software mixer
//! worker, and engine input injected from family events.
//!
//! The Artemis path is in-process Rust and window-free. Unlike the Siglus
//! family there is no engine-side mixer: the core only emits media commands
//! on the host-events queue, so this crate owns decoding (symphonia) and
//! mixing, and reports media completion back to the runtime.

pub mod audio;
pub mod error;
pub mod events;
pub mod media;
pub mod provider;
pub mod session;

pub use provider::{artemis_descriptor, ArtemisProvider};
