use super::*;
use crate::{parse_sc, ScOpcodeCatalog};

fn machine(source: &[u8]) -> MusicaVm {
    MusicaVm::new(
        "musica:/scr/movie.sc".into(),
        Hash256::from_sha256(source),
        parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
        7,
    )
    .unwrap()
}

#[test]
fn movie_cursor_restores_and_completion_clears_the_media_wait() {
    let source = b".movie 9989 op.avi 1280 720 t\r\n.end\r\n";
    let mut vm = machine(source);
    let Some(MusicaVmEvent::Movie(movie)) = vm.step(1).unwrap() else {
        panic!("expected movie")
    };
    assert_eq!(movie.resource_uri, "musica:/mov/op.avi");
    assert_eq!(
        (movie.width, movie.height, movie.skippable),
        (1280, 720, true)
    );
    vm.update_movie_position(&movie.media_id, &movie.fence_id, 123456)
        .unwrap();
    vm.advance_waiting_tick(2).unwrap();
    let saved = vm.encode_native_save().unwrap();
    let mut restored = machine(source);
    restored.restore_native_save(&saved, 10).unwrap();
    assert_eq!(
        restored.state().movie.as_ref().unwrap().continuation_pts,
        123456
    );
    let before = restored.state().clone();
    assert!(restored
        .update_movie_position("musica.movie.1", &movie.fence_id, 200000)
        .is_err());
    assert!(restored
        .update_movie_position(&movie.media_id, "old-fence", 200000)
        .is_err());
    assert!(restored
        .update_movie_position(&movie.media_id, &movie.fence_id, 123455)
        .is_err());
    assert!(restored.resolve_wait("old-fence").is_err());
    assert_eq!(restored.state(), &before);
    restored.resolve_wait(&movie.fence_id).unwrap();
    assert!(restored.state().movie.is_none());
    assert!(restored.state().wait.is_none());
    assert_eq!(restored.step(10).unwrap(), Some(MusicaVmEvent::Terminal));
}

#[test]
fn movie_restore_rejects_corrupt_identity_resource_and_wait_without_replacing_state() {
    let source = b".movie 1 ending.avi 800 600 f\r\n.end\r\n";
    let mut vm = machine(source);
    vm.step(1).unwrap();
    let before = vm.state().clone();
    for variant in 0..7 {
        let mut corrupt = before.clone();
        match variant {
            0 => corrupt.movie.as_mut().unwrap().width = 0,
            1 => corrupt.movie.as_mut().unwrap().resource_uri = "musica:/mov/../secret".into(),
            2 => corrupt.movie.as_mut().unwrap().media_id = "musica.movie.01".into(),
            3 => corrupt.movie.as_mut().unwrap().fence_id = "old-fence".into(),
            4 => corrupt.wait = None,
            5 => corrupt.movie = None,
            _ => corrupt.terminal = true,
        }
        let bytes = postcard::to_allocvec(&corrupt).unwrap();
        assert!(MusicaVm::decode_native_save(&bytes).is_err());
        assert!(vm.restore_native_save(&bytes, 2).is_err());
        assert_eq!(vm.state(), &before);
    }
}

#[test]
fn movie_operands_fail_before_installing_a_partial_movie() {
    for operands in [
        "0 op.avi 800 600 t",
        "1 ../op.avi 800 600 t",
        "1 op.avi 8193 600 t",
        "1 op.avi 800 0 t",
        "1 op.avi 800 600 yes",
        "1 op.avi 800 600",
        "1 op.avi 800 600 t extra",
    ] {
        let mut vm = machine(format!(".movie {operands}\r\n.end\r\n").as_bytes());
        assert!(vm.step(1).is_err());
        assert!(vm.state().movie.is_none());
        assert!(vm.state().wait.is_none());
    }
}
