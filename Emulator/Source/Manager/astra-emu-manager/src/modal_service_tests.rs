use super::*;
use std::{sync::mpsc, thread, time::Duration};

fn request(input: bool) -> ModalRequest {
    ModalRequest {
        caption: "選択".into(),
        message: "原生服务".into(),
        buttons: vec!["継続".into(), "戻る".into()],
        initial_text: if input { Some("初期".into()) } else { None },
    }
}

fn bridge(epoch: u64) -> (ModalBridge, mpsc::Receiver<()>) {
    let (sender, receiver) = mpsc::channel();
    let bridge = ModalBridge::new(
        epoch,
        Arc::new(move || {
            let _ = sender.send(());
        }),
    )
    .unwrap();
    (bridge, receiver)
}

fn presented(bridge: &ModalBridge, receiver: &mpsc::Receiver<()>) -> ModalId {
    receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    match bridge.take_ui_update().unwrap() {
        ModalUiUpdate::Show(value) => value.id,
        ModalUiUpdate::Dismiss(_) => panic!("unexpected dismissal"),
    }
}

#[test]
fn invalid_answer_cannot_complete_pending_input_and_duplicate_requests_are_rejected() {
    let (bridge, wake) = bridge(7);
    let worker = bridge.clone();
    let pending = thread::spawn(move || worker.request(request(true)));
    let id = presented(&bridge, &wake);
    assert!(bridge.request(request(false)).is_err());
    assert!(bridge
        .respond(
            id,
            ModalResponse::Submitted {
                button_index: 2,
                text: Some("戻る".into())
            }
        )
        .is_err());
    assert!(bridge
        .respond(
            id,
            ModalResponse::Submitted {
                button_index: 1,
                text: None
            }
        )
        .is_err());
    assert!(bridge.take_ui_update().is_none());
    let answer = ModalResponse::Submitted {
        button_index: 1,
        text: Some("修改后的文本".into()),
    };
    bridge.respond(id, answer.clone()).unwrap();
    assert_eq!(pending.join().unwrap().unwrap(), answer);
    assert!(bridge.respond(id, ModalResponse::Cancelled).is_err());
    assert!(matches!(bridge.take_ui_update(), Some(ModalUiUpdate::Dismiss(value)) if value == id));
}

#[test]
fn cancellation_wakes_blocked_abi_request_and_old_answers_cannot_enter_new_epoch() {
    let (old, wake) = bridge(11);
    let sink = old.clone();
    let pending = thread::spawn(move || sink.request(request(false)));
    let old_id = presented(&old, &wake);
    old.cancel_pending();
    assert_eq!(pending.join().unwrap().unwrap(), ModalResponse::Cancelled);
    assert!(old
        .respond(
            old_id,
            ModalResponse::Submitted {
                button_index: 0,
                text: None
            }
        )
        .is_err());
    assert_eq!(
        old.request(request(true)).unwrap(),
        ModalResponse::Cancelled
    );
    let (new, wake) = bridge(12);
    let worker = new.clone();
    let pending = thread::spawn(move || worker.request(request(false)));
    let new_id = presented(&new, &wake);
    assert!(new.respond(old_id, ModalResponse::Cancelled).is_err());
    new.respond(new_id, ModalResponse::Cancelled).unwrap();
    assert_eq!(pending.join().unwrap().unwrap(), ModalResponse::Cancelled);
}

#[test]
fn consecutive_requests_preserve_dismissal_then_new_presentation() {
    let (bridge, wake) = bridge(19);
    let worker = bridge.clone();
    let first = thread::spawn(move || worker.request(request(false)));
    let first_id = presented(&bridge, &wake);
    bridge.respond(first_id, ModalResponse::Cancelled).unwrap();
    first.join().unwrap().unwrap();
    wake.recv_timeout(Duration::from_secs(5)).unwrap();
    let worker = bridge.clone();
    let second = thread::spawn(move || worker.request(request(true)));
    wake.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(bridge.take_ui_update(), Some(ModalUiUpdate::Dismiss(id)) if id == first_id));
    let second_id = match bridge.take_ui_update().unwrap() {
        ModalUiUpdate::Show(value) => value.id,
        ModalUiUpdate::Dismiss(_) => panic!("unexpected second dismissal"),
    };
    assert_eq!(second_id.sequence, first_id.sequence + 1);
    bridge.cancel_pending();
    assert_eq!(second.join().unwrap().unwrap(), ModalResponse::Cancelled);
}
