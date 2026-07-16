use astra_platform::{PlatformError, PlatformErrorCode};

use crate::{ANDROID_INVALID_LIFECYCLE, ANDROID_WINDOW_ALREADY_CREATED};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidLifecycleState {
    Created,
    Started,
    Resumed,
    Paused,
    Stopped,
    Destroyed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidLifecycleEvent {
    Start,
    Resume,
    Pause,
    Stop,
    Destroy,
}

#[derive(Debug)]
pub struct AndroidLifecycle {
    state: AndroidLifecycleState,
    window_created: bool,
    native_window_available: bool,
}

impl Default for AndroidLifecycle {
    fn default() -> Self {
        Self {
            state: AndroidLifecycleState::Created,
            window_created: false,
            native_window_available: false,
        }
    }
}

impl AndroidLifecycle {
    pub fn state(&self) -> AndroidLifecycleState {
        self.state
    }

    pub fn transition(&mut self, event: AndroidLifecycleEvent) -> Result<(), PlatformError> {
        use AndroidLifecycleEvent as Event;
        use AndroidLifecycleState as State;
        self.state = match (self.state, event) {
            (State::Created, Event::Start) | (State::Stopped, Event::Start) => State::Started,
            (State::Started, Event::Resume) | (State::Paused, Event::Resume) => State::Resumed,
            (State::Resumed, Event::Pause) => State::Paused,
            (State::Paused, Event::Stop) | (State::Started, Event::Stop) => State::Stopped,
            (
                State::Created | State::Started | State::Resumed | State::Paused | State::Stopped,
                Event::Destroy,
            ) => State::Destroyed,
            _ => {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "android.lifecycle.transition",
                    "Android lifecycle transition is invalid",
                )
                .with_field("diagnostic_code", ANDROID_INVALID_LIFECYCLE));
            }
        };
        Ok(())
    }

    pub fn set_native_window_available(&mut self, available: bool) {
        self.native_window_available = available;
    }

    pub fn create_main_window(&mut self) -> Result<(), PlatformError> {
        if self.window_created {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "window.create",
                "Android Activity owns exactly one main window",
            )
            .with_field("diagnostic_code", ANDROID_WINDOW_ALREADY_CREATED));
        }
        if !self.native_window_available || self.state != AndroidLifecycleState::Resumed {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "window.create",
                "Android main window requires a resumed Activity and native window",
            )
            .with_field("diagnostic_code", ANDROID_INVALID_LIFECYCLE));
        }
        self.window_created = true;
        Ok(())
    }

    pub fn destroy_main_window(&mut self) -> Result<(), PlatformError> {
        if !self.window_created {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "window.destroy",
                "Android main window is not live",
            ));
        }
        self.window_created = false;
        Ok(())
    }
}
