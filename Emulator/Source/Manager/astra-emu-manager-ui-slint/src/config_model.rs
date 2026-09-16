use std::cell::RefCell;

use slint::{Model, ModelRc, VecModel};

use crate::{ConfigField, GenericConfigFieldViewModel};

pub(crate) fn update(
    model: &VecModel<ConfigField>,
    previous: &RefCell<Vec<GenericConfigFieldViewModel>>,
    fields: &[GenericConfigFieldViewModel],
) {
    let mut previous = previous.borrow_mut();
    if previous.as_slice() == fields {
        return;
    }
    if previous.len() != fields.len() || previous.iter().zip(fields).any(|(a, b)| a.key != b.key) {
        model.set_vec(fields.iter().map(convert).collect::<Vec<_>>());
    } else {
        for (index, (old, new)) in previous.iter().zip(fields).enumerate() {
            if old != new {
                let mut field = convert(new);
                if old.enum_values == new.enum_values {
                    field.options = model.row_data(index).expect("existing config row").options;
                }
                model.set_row_data(index, field);
            }
        }
    }
    fields.clone_into(&mut previous);
}

fn convert(field: &GenericConfigFieldViewModel) -> ConfigField {
    ConfigField {
        options: ModelRc::new(VecModel::from(
            field
                .enum_values
                .iter()
                .map(|value| value.as_str().into())
                .collect::<Vec<_>>(),
        )),
        key: field.key.as_str().into(),
        label: field.label.as_str().into(),
        description: field.description.as_str().into(),
        kind: field.kind.as_str().into(),
        value: field.value.as_str().into(),
        required: field.required,
        min: field.min,
        max: field.max,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_preserves_enum_model_and_applies_changed_values() {
        let model = VecModel::default();
        let previous = RefCell::default();
        let mut fields = vec![GenericConfigFieldViewModel {
            key: "encoding".into(),
            label: "Encoding".into(),
            description: String::new(),
            kind: "enum".into(),
            value: "sjis".into(),
            enum_values: vec!["sjis".into(), "gbk".into()],
            required: true,
            min: 0,
            max: 0,
        }];
        update(&model, &previous, &fields);
        let options = model.row_data(0).unwrap().options;
        update(&model, &previous, &fields);
        assert_eq!(model.row_data(0).unwrap().options, options);
        fields[0].value = "gbk".into();
        update(&model, &previous, &fields);
        assert_eq!(model.row_data(0).unwrap().value, "gbk");
        assert_eq!(model.row_data(0).unwrap().options, options);
        fields[0].enum_values.push("utf8".into());
        update(&model, &previous, &fields);
        assert_eq!(model.row_data(0).unwrap().options.row_count(), 3);
        update(&model, &previous, &[]);
        assert_eq!(model.row_count(), 0);
    }
}
