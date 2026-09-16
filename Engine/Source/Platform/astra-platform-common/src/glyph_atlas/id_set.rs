use std::collections::HashSet;

use smallvec::SmallVec;

/// Frame-local membership only; render order remains in the command stream.
#[derive(Default)]
pub(super) struct SceneIdSet<'a> {
    inline: SmallVec<[&'a str; 128]>,
    large: Option<HashSet<&'a str>>,
}

impl<'a> SceneIdSet<'a> {
    pub(super) fn contains(&self, value: &str) -> bool {
        match &self.large {
            Some(ids) => ids.contains(value),
            None => self.inline.contains(&value),
        }
    }

    pub(super) fn insert(&mut self, value: &'a str) -> bool {
        if let Some(ids) = &mut self.large {
            return ids.insert(value);
        }
        if self.inline.contains(&value) {
            return false;
        }
        if self.inline.len() < self.inline.inline_size() {
            self.inline.push(value);
        } else {
            let mut ids = HashSet::with_capacity(self.inline.len() * 2);
            ids.extend(self.inline.drain(..));
            ids.insert(value);
            self.large = Some(ids);
        }
        true
    }

    pub(super) fn len(&self) -> usize {
        self.large.as_ref().map_or(self.inline.len(), HashSet::len)
    }

    pub(super) fn spilled(&self) -> bool {
        self.large.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::SceneIdSet;

    #[test]
    fn frame_ids_preserve_membership_across_inline_and_large_scenes() {
        let names: Vec<_> = (0..16_384).map(|i| format!("scene.{i}")).collect();
        let mut ids = SceneIdSet::default();
        for (index, name) in names.iter().enumerate() {
            assert!(ids.insert(name));
            assert!(!ids.insert(name));
            assert_eq!(ids.len(), index + 1);
            assert_eq!(ids.spilled(), index >= 128);
        }
        for name in &names {
            assert!(ids.contains(name));
            assert!(!ids.insert(name));
        }
        assert!(!ids.contains("absent"));
        assert_eq!(ids.len(), names.len());
    }
}
