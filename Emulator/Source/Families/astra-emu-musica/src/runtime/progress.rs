use super::*;
const FLAGS: [&str; 4] = ["TOHKA_CLEAR", "AYAME_CLEAR", "SUI_CLEAR", "REN_CLEAR"];
pub(crate) fn validate(unlocks: &[Hash256]) -> Result<(), MusicaRuntimeError> {
    let known = FLAGS.map(|flag| Hash256::from_sha256(flag.as_bytes()));
    if unlocks.len() > known.len()
        || unlocks.windows(2).any(|p| p[0] >= p[1])
        || unlocks.iter().any(|id| !known.contains(id))
    {
        return Err(MusicaRuntimeError::State);
    }
    Ok(())
}
pub(super) fn record(state: &mut MusicaRuntimeState, key: &str, value: i64) {
    if value != 1 || !FLAGS.contains(&key) {
        return;
    }
    let id = Hash256::from_sha256(key.as_bytes());
    if let Err(index) = state.gallery_unlocks.binary_search(&id) {
        state.gallery_unlocks.insert(index, id);
    }
}
impl MusicaVm {
    pub fn merge_verified_gallery_unlocks(
        &mut self,
        unlocks: &[Hash256],
    ) -> Result<(), MusicaRuntimeError> {
        validate(unlocks)?;
        for flag in FLAGS {
            let id = Hash256::from_sha256(flag.as_bytes());
            if unlocks.contains(&id) {
                self.state.global_variables.insert(flag.into(), 1);
                record(&mut self.state, flag, 1);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gallery_progress_records_only_verified_clear_values_and_rejects_corrupt_saves() {
        let source = b".setglobal REN_CLEAR = 0\r\n.setglobal OTHER_CLEAR = 1\r\n.setglobal AYAME_CLEAR = 1\r\n.setglobal AYAME_CLEAR = 1\r\n.end\r\n";
        let mut vm = MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            crate::parse_sc(source, &crate::ScOpcodeCatalog::observed_musica()).unwrap(),
            0,
        )
        .unwrap();
        vm.step(1).unwrap();
        let id = Hash256::from_sha256(b"AYAME_CLEAR");
        assert_eq!(vm.state().gallery_unlocks, vec![id]);
        let saved = vm.encode_native_save().unwrap();
        let mut corrupt = vm.state().clone();
        corrupt.gallery_unlocks.push(id);
        let bytes = postcard::to_allocvec(&corrupt).unwrap();
        assert!(MusicaVm::decode_native_save(&bytes).is_err());
        assert!(vm.restore_native_save(&bytes, 1).is_err());
        assert_eq!(vm.encode_native_save().unwrap(), saved);
        assert!(vm
            .merge_verified_gallery_unlocks(&[Hash256::from_sha256(b"UNKNOWN")])
            .is_err());
        vm.merge_verified_gallery_unlocks(&[Hash256::from_sha256(b"TOHKA_CLEAR")])
            .unwrap();
        assert_eq!(vm.state().global_variables["TOHKA_CLEAR"], 1);
        assert_eq!(vm.state().gallery_unlocks.len(), 2);
    }
}
