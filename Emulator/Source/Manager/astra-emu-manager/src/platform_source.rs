#[cfg(target_os = "android")]
pub use crate::android_source::{
    AndroidGrantedSource as GrantedSource, AndroidVfsRegistry as VfsRegistry,
};

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
pub use astra_emu_manager::desktop_source::{
    DesktopGrantedSource as GrantedSource, DesktopVfsRegistry as VfsRegistry,
};

#[cfg(not(any(
    target_os = "windows",
    target_os = "linux",
    target_os = "macos",
    target_os = "android"
)))]
pub use crate::unsupported_source::{
    UnsupportedGrantedSource as GrantedSource, UnsupportedVfsRegistry as VfsRegistry,
};
