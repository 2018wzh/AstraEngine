use std::cell::RefCell;

use slint::{Model, ModelRc, SharedString, VecModel};

use crate::{ConfigField, ConfigFieldViewModel};

pub(crate) fn update(
    model: &VecModel<ConfigField>,
    previous: &RefCell<Vec<ConfigFieldViewModel>>,
    fields: &[ConfigFieldViewModel],
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
                let existing = model.row_data(index).expect("existing config row");
                if old.options == new.options {
                    field.options = existing.options;
                }
                if old.option_values == new.option_values {
                    field.option_values = existing.option_values;
                }
                model.set_row_data(index, field);
            }
        }
    }
    fields.clone_into(&mut previous);
}

fn strings(values: &[String]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        values
            .iter()
            .map(|value| SharedString::from(value.as_str()))
            .collect::<Vec<_>>(),
    ))
}

fn convert(field: &ConfigFieldViewModel) -> ConfigField {
    ConfigField {
        key: field.key.as_str().into(),
        label: field.label.as_str().into(),
        description: field.description.as_str().into(),
        group: field.group.as_str().into(),
        kind: field.kind.as_str().into(),
        value: field.value.as_str().into(),
        display_value: field.display_value.as_str().into(),
        options: strings(&field.options),
        option_values: strings(&field.option_values),
        min: field.min as f32,
        max: field.max as f32,
        step: field.step as f32,
        unit: field.unit.as_str().into(),
        advanced: field.advanced,
        overridden: field.overridden,
        error: field.error.as_str().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_preserves_enum_models_and_applies_changed_values() {
        let model = VecModel::default();
        let previous = RefCell::default();
        let mut fields = vec![ConfigFieldViewModel {
            key: "script_encoding".into(),
            label: "脚本编码".into(),
            kind: "enum".into(),
            value: "shift_jis".into(),
            display_value: "Shift-JIS（日文原版）".into(),
            options: vec!["Shift-JIS（日文原版）".into(), "GBK（简体中文）".into()],
            option_values: vec!["shift_jis".into(), "gbk".into()],
            ..ConfigFieldViewModel::default()
        }];
        update(&model, &previous, &fields);
        let options = model.row_data(0).unwrap().options;
        let values = model.row_data(0).unwrap().option_values;
        update(&model, &previous, &fields);
        assert_eq!(model.row_data(0).unwrap().options, options);
        fields[0].value = "gbk".into();
        fields[0].overridden = true;
        update(&model, &previous, &fields);
        let row = model.row_data(0).unwrap();
        assert_eq!(row.value, "gbk");
        assert!(row.overridden);
        assert_eq!(row.options, options);
        assert_eq!(row.option_values, values);
        fields[0].options.push("UTF-8".into());
        fields[0].option_values.push("utf8".into());
        update(&model, &previous, &fields);
        assert_eq!(model.row_data(0).unwrap().options.row_count(), 3);
        assert_eq!(model.row_data(0).unwrap().option_values.row_count(), 3);
        update(&model, &previous, &[]);
        assert_eq!(model.row_count(), 0);
    }
}
