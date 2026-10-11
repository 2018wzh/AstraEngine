//! KrKr Family v11 adapter and its owned native child transport.
//! Each session uses a fresh child and the real native owner DLL. This crate
//! remains outside the Manager registry until native integration is complete.

pub mod child;
pub mod family;
pub mod parent;
mod sockets;
pub mod wire;
