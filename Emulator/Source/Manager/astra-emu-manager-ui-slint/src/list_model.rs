use std::{cell::RefCell, rc::Rc};

use slint::{Model, ModelRc, VecModel};

/// Keeps one Slint model per list and touches only rows whose view value changed,
/// so repeaters keep their instances (and any focus or popup state) across applies.
pub(crate) struct ListModel<V, T: 'static> {
    rows: Rc<VecModel<T>>,
    previous: RefCell<Vec<V>>,
    convert: fn(&V) -> T,
}

impl<V: PartialEq + Clone, T: Clone + 'static> ListModel<V, T> {
    pub(crate) fn new(convert: fn(&V) -> T) -> Self {
        Self {
            rows: Rc::new(VecModel::default()),
            previous: RefCell::new(Vec::new()),
            convert,
        }
    }

    pub(crate) fn model(&self) -> ModelRc<T> {
        ModelRc::from(self.rows.clone())
    }

    pub(crate) fn update(&self, items: &[V]) {
        let mut previous = self.previous.borrow_mut();
        if previous.as_slice() == items {
            return;
        }
        if previous.len() == items.len() {
            for (index, (old, new)) in previous.iter().zip(items).enumerate() {
                if old != new {
                    self.rows.set_row_data(index, (self.convert)(new));
                }
            }
        } else {
            self.rows
                .set_vec(items.iter().map(self.convert).collect::<Vec<_>>());
        }
        items.clone_into(&mut previous);
    }
}

impl ListModel<String, slint::SharedString> {
    pub(crate) fn strings() -> Self {
        Self::new(|value| value.as_str().into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_rows_are_not_rewritten() {
        let list = ListModel::strings();
        list.update(&["a".to_string(), "b".to_string()]);
        let model = list.model();
        list.update(&["a".to_string(), "c".to_string()]);
        assert_eq!(model.row_count(), 2);
        assert_eq!(model.row_data(1).unwrap(), "c");
        list.update(&["a".to_string()]);
        assert_eq!(model.row_count(), 1);
        list.update(&[]);
        assert_eq!(model.row_count(), 0);
    }
}
