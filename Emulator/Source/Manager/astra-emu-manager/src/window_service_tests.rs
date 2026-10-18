use super::*;
use std::{sync::mpsc, time::Duration};
fn bridge() -> (WindowBridge, mpsc::Receiver<()>) {
    let (send, receive) = mpsc::channel();
    let wake: HostWake = Arc::new(move || {
        let _ = send.send(());
    });
    (
        WindowBridge::new(1, ModalBridge::new(1, wake.clone()).unwrap(), wake),
        receive,
    )
}
fn raw(kind: u32) -> abi::WindowRequest {
    let mut value: abi::WindowRequest = unsafe { std::mem::zeroed() };
    value.kind = kind;
    value.session_epoch = 1;
    value.context_epoch = 1;
    value.request_id = 1;
    value
}
fn result() -> abi::WindowResult {
    unsafe { std::mem::zeroed() }
}
#[test]
fn input_capacity_is_checked_before_modal_presentation() {
    let (bridge, wake) = bridge();
    let service = bridge.sink();
    let mut request = raw(abi::WINDOW_MODAL);
    request.flags = abi::MODAL_TEXT_INPUT;
    let mut output = result();
    assert_eq!(
        unsafe {
            service.request.unwrap()(
                service.context,
                &request,
                &mut output,
                &mut abi::Error::default(),
            )
        },
        abi::CAPACITY
    );
    assert_eq!(output.input.len, abi::MAX_MODAL_TEXT);
    assert!(wake.try_recv().is_err());
    bridge.close();
}
#[test]
fn modal_input_preserves_caption_initial_text_and_explicit_button_id() {
    let (bridge, wake) = bridge();
    let worker_bridge = bridge.clone();
    let worker = std::thread::spawn(move || {
        let service = worker_bridge.sink();
        let mut request = raw(abi::WINDOW_MODAL);
        request.flags = abi::MODAL_TEXT_INPUT;
        request.caption = abi::Bytes::new(b"Caption");
        request.text = abi::Bytes::new(b"Question");
        request.initial_text = abi::Bytes::new(b"Initial");
        let button = abi::WindowButton {
            id: 42,
            reserved: 0,
            label: abi::Bytes::new(b"Accept"),
        };
        request.buttons = &button;
        request.button_count = 1;
        let mut bytes = vec![0; abi::MAX_MODAL_TEXT];
        let mut output = result();
        output.input = abi::TextBuffer {
            data: bytes.as_mut_ptr(),
            capacity: bytes.len(),
            len: 0,
        };
        let status = unsafe {
            service.request.unwrap()(
                service.context,
                &request,
                &mut output,
                &mut abi::Error::default(),
            )
        };
        (status, output.selected_button, output.input.len, bytes)
    });
    wake.recv_timeout(Duration::from_secs(5)).unwrap();
    let Some(crate::modal_service::ModalUiUpdate::Show(presentation)) =
        bridge.0.modal.take_ui_update()
    else {
        panic!("missing input modal")
    };
    assert_eq!(presentation.request.caption, "Caption");
    assert_eq!(
        presentation.request.initial_text.as_deref(),
        Some("Initial")
    );
    bridge
        .0
        .modal
        .respond(
            presentation.id,
            ModalResponse::Submitted {
                button_index: 0,
                text: Some("Answer".into()),
            },
        )
        .unwrap();
    let (status, button, count, bytes) = worker.join().unwrap();
    assert_eq!(status, abi::OK);
    assert_eq!(button, 42);
    assert_eq!(&bytes[..count], b"Answer");
    bridge.close();
}
#[test]
fn cancellation_before_registration_and_context_invalidation_do_not_lose_wakeups() {
    let (bridge, _) = bridge();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let worker_barrier = barrier.clone();
    let worker_bridge = bridge.clone();
    let worker = std::thread::spawn(move || {
        worker_barrier.wait();
        let service = worker_bridge.sink();
        let mut output = result();
        unsafe {
            service.request.unwrap()(
                service.context,
                &raw(abi::WINDOW_TITLE),
                &mut output,
                &mut abi::Error::default(),
            )
        }
    });
    bridge.close();
    barrier.wait();
    assert_eq!(worker.join().unwrap(), abi::CANCELLED);
}
#[test]
fn invalidation_wakes_window_request_without_platform_response() {
    let (bridge, wake) = bridge();
    let worker_bridge = bridge.clone();
    let worker = std::thread::spawn(move || {
        let service = worker_bridge.sink();
        let mut output = result();
        unsafe {
            service.request.unwrap()(
                service.context,
                &raw(abi::WINDOW_TITLE),
                &mut output,
                &mut abi::Error::default(),
            )
        }
    });
    wake.recv_timeout(Duration::from_secs(5)).unwrap();
    let request = bridge.take().unwrap();
    let service = bridge.sink();
    unsafe {
        service.invalidate.unwrap()(service.context, 1, 2);
    }
    assert_eq!(worker.join().unwrap(), abi::CANCELLED);
    assert!(bridge
        .respond(&request, Ok(WindowOutcome::default()))
        .is_err());
    bridge.close();
}
