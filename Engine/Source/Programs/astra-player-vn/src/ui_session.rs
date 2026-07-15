use std::collections::BTreeMap;

use astra_ui_core::UiValue;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActiveUiController {
    pub instance_id: String,
    pub controller_id: String,
    pub view_id: String,
    pub model_schema: String,
    pub model: UiValue,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActiveUiModal {
    pub instance_id: String,
    pub controller_id: String,
    pub view_id: String,
    pub model_schema: String,
    pub model: UiValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveUiAnimation {
    pub target_id: String,
    pub preset_id: String,
    pub started_at_ns: u64,
    pub duration_ns: u64,
}

impl ActiveUiAnimation {
    pub fn progress_millionths(&self, fixed_time_ns: u64) -> u64 {
        if self.duration_ns == 0 {
            return 1_000_000;
        }
        fixed_time_ns
            .saturating_sub(self.started_at_ns)
            .saturating_mul(1_000_000)
            .checked_div(self.duration_ns)
            .unwrap_or(1_000_000)
            .min(1_000_000)
    }
}

pub(crate) fn controller_state_value(
    state: &BTreeMap<String, UiValue>,
    animations: impl Iterator<Item = (String, u64)>,
) -> UiValue {
    let mut root = state.clone();
    let animation_values = animations
        .map(|(target, progress)| (target, UiValue::Integer(progress as i64)))
        .collect::<BTreeMap<_, _>>();
    root.insert(
        "astra_animation_progress".into(),
        UiValue::Map(animation_values),
    );
    UiValue::Map(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[astra_headless_test::test]
    fn animation_progress_uses_only_fixed_time() {
        let animation = ActiveUiAnimation {
            target_id: "root/button".into(),
            preset_id: "motion.open".into(),
            started_at_ns: 100,
            duration_ns: 400,
        };
        assert_eq!(animation.progress_millionths(100), 0);
        assert_eq!(animation.progress_millionths(300), 500_000);
        assert_eq!(animation.progress_millionths(900), 1_000_000);
    }
}
