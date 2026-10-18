use astra_emu_manager::effects::PreparedFilter;

/// A deferred request is attempted once. Failure blocks presentation until an
/// explicit configuration replaces it, and reports its original error once.
#[derive(Default)]
pub(super) enum FilterActivation {
    #[default]
    Ready,
    Pending(PreparedFilter),
    Failed(Option<String>),
}

impl FilterActivation {
    pub(super) fn defer(&mut self, prepared: PreparedFilter) {
        *self = Self::Pending(prepared);
    }

    pub(super) fn take_pending(&mut self) -> Option<PreparedFilter> {
        if !matches!(self, Self::Pending(_)) {
            return None;
        }
        // Keep presentation blocked while the request is being activated.
        match std::mem::replace(self, Self::Failed(None)) {
            Self::Pending(prepared) => Some(prepared),
            _ => unreachable!(),
        }
    }

    pub(super) fn complete(&mut self, result: Result<(), String>) {
        *self = match result {
            Ok(()) => Self::Ready,
            Err(error) => Self::Failed(Some(error)),
        };
    }

    pub(super) fn ready_for_frame(&mut self) -> Result<bool, String> {
        match self {
            Self::Ready => Ok(true),
            Self::Pending(_) => Ok(false),
            Self::Failed(error) => error.take().map_or(Ok(false), Err),
        }
    }
}
