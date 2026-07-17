#[cfg(feature = "no_std")]
use alloc::collections::{BTreeMap, BTreeSet};
#[cfg(not(feature = "no_std"))]
use std::collections::{BTreeMap, BTreeSet};

pub type StableHashMap<K, V> = BTreeMap<K, V>;

pub type StableHashSet<T> = BTreeSet<T>;

#[cfg(test)]
mod tests {
    use super::StableHashMap;

    #[test]
    fn serialization_order_does_not_depend_on_insertion_order() {
        let mut left = StableHashMap::new();
        left.insert("second", 2_u8);
        left.insert("first", 1_u8);
        let mut right = StableHashMap::new();
        right.insert("first", 1_u8);
        right.insert("second", 2_u8);
        assert_eq!(
            bincode::serialize(&left).unwrap(),
            bincode::serialize(&right).unwrap()
        );
    }
}
