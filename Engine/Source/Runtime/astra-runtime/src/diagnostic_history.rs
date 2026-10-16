use std::collections::VecDeque;

pub(crate) const CAPACITY: usize = 1024;

/// Diagnostic records are not execution input or recovery state.
pub(crate) fn record<T>(history: &mut VecDeque<T>, value: T) {
    if history.len() == CAPACITY {
        history.pop_front();
    }
    history.push_back(value);
}

pub(crate) fn extend<T>(history: &mut VecDeque<T>, values: impl IntoIterator<Item = T>) {
    for value in values {
        record(history, value);
    }
}
