use crate::{Bytes, Header, Status, UNSUPPORTED};

pub const SESSION_MENU_EXTENSION: &[u8] = b"astra.emu.session_menu";
pub const SESSION_MENU_VERSION: u32 = 1;
/// Core-defined control commands start here; built-in CONTROL_* stay below.
pub const CONTROL_CORE_BASE: u32 = 0x1000;
pub const SESSION_MENU_MAX_ITEMS: usize = 16;
pub const SESSION_MENU_LABEL_MAX: usize = 64;
pub const MENU_ICON_GENERIC: u32 = 0;
pub const MENU_ICON_QUICK_SAVE: u32 = 1;
pub const MENU_ICON_QUICK_LOAD: u32 = 2;
pub const MENU_ICON_SAVE: u32 = 3;
pub const MENU_ICON_LOAD: u32 = 4;
pub const MENU_ICON_AUTO: u32 = 5;
pub const MENU_ICON_SKIP: u32 = 6;
pub const MENU_ICON_BACKLOG: u32 = 7;
pub const MENU_ICON_SETTINGS: u32 = 8;

/// One Host-invoked session control. `control` is unique and
/// `>= CONTROL_CORE_BASE`. `label` is library-lifetime UTF-8.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SessionMenuItem {
    pub control: u32,
    pub icon: u32,
    pub label: Bytes,
}

/// Core-defined in-session menu. The table and labels are library-lifetime,
/// immutable, UTF-8. Invoking an item means the Host calls
/// `CoreApi.control(session, item.control)`. Controls are `>= CONTROL_CORE_BASE`
/// and unique. A core returns `INVALID_STATE` when the action is not possible
/// now; this is not a session failure.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SessionMenuApi {
    pub header: Header,
    pub items: *const SessionMenuItem,
    pub count: usize,
}

impl SessionMenuApi {
    pub const HEADER: Header = Header {
        size: std::mem::size_of::<Self>() as u32,
        version: SESSION_MENU_VERSION,
    };

    pub fn validate(&self) -> Result<(), Status> {
        if self.header.version != SESSION_MENU_VERSION
            || self.header.size < Self::HEADER.size
            || self.count > SESSION_MENU_MAX_ITEMS
            || (self.count > 0 && (self.items.is_null() || !self.items.is_aligned()))
        {
            return Err(UNSUPPORTED);
        }
        Ok(())
    }
}

// SAFETY: they only point to immutable static storage.
unsafe impl Sync for SessionMenuItem {}
unsafe impl Sync for SessionMenuApi {}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_item() -> SessionMenuItem {
        SessionMenuItem {
            control: CONTROL_CORE_BASE,
            icon: MENU_ICON_GENERIC,
            label: Bytes::from_static(b"Action"),
        }
    }

    #[test]
    fn valid_table_passes() {
        let items = [valid_item()];
        let api = SessionMenuApi {
            header: SessionMenuApi::HEADER,
            items: items.as_ptr(),
            count: 1,
        };
        assert_eq!(api.validate(), Ok(()));
    }

    #[test]
    fn count_above_max_is_unsupported() {
        let items = [valid_item()];
        let api = SessionMenuApi {
            header: SessionMenuApi::HEADER,
            items: items.as_ptr(),
            count: 17,
        };
        assert_eq!(api.validate(), Err(UNSUPPORTED));
    }

    #[test]
    fn wrong_version_is_unsupported() {
        let items = [valid_item()];
        let mut api = SessionMenuApi {
            header: SessionMenuApi::HEADER,
            items: items.as_ptr(),
            count: 1,
        };
        api.header.version = 2;
        assert_eq!(api.validate(), Err(UNSUPPORTED));
    }

    #[test]
    fn null_items_with_count_is_unsupported() {
        let api = SessionMenuApi {
            header: SessionMenuApi::HEADER,
            items: std::ptr::null(),
            count: 1,
        };
        assert_eq!(api.validate(), Err(UNSUPPORTED));
    }
}
