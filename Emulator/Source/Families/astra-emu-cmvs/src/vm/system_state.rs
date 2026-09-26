use super::*;

/// Applies the decoded native tables to their live process owners. Validation
/// and list construction finish before any authoritative state changes.
pub fn apply_system_save(
    state: &mut CmvsPs2aVmState,
    save: crate::CmvsSystemSave,
) -> Result<(), CoreError> {
    save.validate()?;
    let mut lists = state.native_system_lists.clone();
    for (index, incoming) in save.lists.iter().enumerate() {
        // sub_427B30 leaves a list alone when its stored count is zero.
        if incoming.is_empty() {
            continue;
        }
        lists[index].clear();
        let gate = [10508, 10512][index];
        if state.interpreter_words.get(&gate).copied().unwrap_or(0) != 0 {
            let mut keys = std::collections::BTreeSet::new();
            for value in incoming {
                if keys.insert(native_string_key(value)) {
                    lists[index].push(value.clone());
                }
            }
        }
    }
    crate::system_save::validate_lists(&lists)?;
    state
        .process_flag_bits
        .retain(|bit| !(2048..262144).contains(bit));
    for (index, byte) in save.flags.iter().enumerate() {
        for bit in 0..8 {
            if byte & (1 << bit) != 0 {
                state
                    .process_flag_bits
                    .insert(2048 + index as u32 * 8 + bit);
            }
        }
    }
    for (index, value) in save.cg_words.into_iter().enumerate() {
        state
            .process_indexed_words
            .insert(2048 + index as u32, value);
    }
    for (index, value) in save.bgm_words.into_iter().enumerate() {
        state.process_float_words.insert(1024 + index as u32, value);
    }
    for (index, value) in save.title_slots.into_iter().enumerate() {
        state.process_string_slots.insert(64 + index as u32, value);
    }
    state.native_system_lists = lists;
    Ok(())
}

// sub_41A420 folds single-byte ASCII only. A Shift-JIS trail byte must
// never be folded even when its numeric value is an ASCII letter.
fn native_string_key(value: &[u8]) -> Vec<u8> {
    let mut result = value.to_vec();
    let mut index = 0;
    while index < result.len() {
        if matches!(result[index], 0x81..=0x9f | 0xe0..=0xfc) {
            index += 2;
        } else {
            result[index] = result[index].to_ascii_uppercase();
            index += 1;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn candidate() -> crate::CmvsSystemSave {
        crate::parse_cmvs_system_save(
            &crate::system_save::tests::encode_container(
                &crate::system_save::tests::layout_payload(),
                0,
            ),
            0,
        )
        .unwrap()
    }

    #[test]
    fn lists_follow_native_enable_count_and_shift_jis_comparison() {
        let mut vm = CmvsPs2aVmState::new(0);
        vm.interpreter_words.insert(10508, 1);
        vm.native_system_lists[1].push(b"old".to_vec());
        let mut save = candidate();
        save.lists[0] = vec![
            b"Alpha".to_vec(),
            b"aLPHA".to_vec(),
            vec![0x83, 0x61],
            vec![0x83, 0x41],
        ];
        save.lists[1] = vec![b"disabled".to_vec()];
        apply_system_save(&mut vm, save).unwrap();
        assert_eq!(
            vm.native_system_lists[0],
            vec![b"Alpha".to_vec(), vec![0x83, 0x61], vec![0x83, 0x41]]
        );
        assert!(vm.native_system_lists[1].is_empty());
        vm.interpreter_words.insert(10508, 0);
        let retained = vm.native_system_lists.clone();
        apply_system_save(&mut vm, candidate()).unwrap();
        assert_eq!(vm.native_system_lists, retained);
    }

    #[test]
    fn malformed_decoded_tables_are_atomic() {
        let mut vm = CmvsPs2aVmState::new(0);
        vm.process_flag_bits.insert(2048);
        let expected = vm.clone();
        let mut save = candidate();
        save.title_slots[63] = vec![b'a'; 1024];
        assert!(apply_system_save(&mut vm, save).is_err());
        assert_eq!(vm, expected);
        let mut save = candidate();
        save.bgm_words.pop();
        assert!(apply_system_save(&mut vm, save).is_err());
        assert_eq!(vm, expected);
    }
}
