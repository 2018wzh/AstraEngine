use super::*;
use parking_lot::Mutex;
use std::sync::Arc;

#[derive(Default)]
struct State {
    ready: bool,
    advances: Vec<u64>,
    completed: Option<Instant>,
}

struct SignaledFrame {
    state: Arc<Mutex<State>>,
    frame: FramePublisher,
    reset: bool,
    dirty: bool,
}

fn info() -> FrameInfo {
    FrameInfo {
        width: 1,
        height: 1,
        logical_width: 1,
        logical_height: 1,
        stride: 4,
        format: FrameFormat::Rgba8Srgb {
            alpha: FrameAlpha::Ignored,
        },
        rows: FrameRowOrder::TopToBottom,
    }
}

impl FamilySession for SignaledFrame {
    fn advance(&mut self, elapsed: u64, _: &[FamilyEvent]) -> FamilyResult<AdvanceResponse> {
        self.state.lock().advances.push(elapsed);
        self.dirty = true;
        Ok(AdvanceResponse {
            reset_clock: std::mem::take(&mut self.reset),
            ..AdvanceResponse::running()
        })
    }

    fn acquire_frame(&mut self, after: Option<FrameIdentity>) -> FamilyResult<FramePoll> {
        if self.dirty {
            if !self.state.lock().ready {
                return Ok(self.frame.pending());
            }
            let value = self.state.lock().advances.len() as u8;
            self.frame
                .publish(vec![value, 17, 29, 255], Vec::as_slice, info())?;
            self.dirty = false;
            self.state.lock().completed = Some(Instant::now());
        }
        Ok(self.frame.poll(after))
    }

    fn close(self: Box<Self>) -> FamilyResult<()> {
        Ok(())
    }
}

fn active(reset: bool, initial_pending: bool) -> (ActiveFamilySession, Arc<Mutex<State>>) {
    let state = Arc::new(Mutex::new(State::default()));
    let mut frame = FramePublisher::new(1).unwrap();
    frame
        .publish(vec![0, 0, 0, 255], Vec::as_slice, info())
        .unwrap();
    let mailbox = FrameMailbox::new();
    mailbox.activate(1).unwrap();
    if !initial_pending {
        let FramePoll::Ready(initial) = frame.poll(None) else {
            unreachable!()
        };
        mailbox.publish(initial).unwrap();
    }
    let now = Instant::now();
    (
        ActiveFamilySession {
            family_id: "pending-clock-test".into(),
            session_epoch: 1,
            session: Some(Box::new(SignaledFrame {
                state: Arc::clone(&state),
                frame,
                reset,
                dirty: initial_pending,
            })),
            audio: None,
            text: None,
            modal: None,
            mailbox,
            status: FamilyStatus::Running,
            fullscreen: false,
            title: None,
            pending_icon: None,
            pending_pointer: None,
            last_tick: now,
            next_deadline: now + Duration::from_nanos(FIXED_FRAME_NS),
            frame_poll_deadline: None,
            clock_reset_pending: false,
        },
        state,
    )
}

fn service(active: &mut ActiveFamilySession, now: Instant) -> bool {
    crate::manager_app::owner_worker::service_due(active, &mut Vec::new(), now).unwrap()
}

#[test]
fn completed_frame_is_published_between_logic_ticks_without_advancing_vm() {
    let (mut active, state) = active(false, false);
    active.advance(1, &[]).unwrap();
    let anchor = active.last_tick;
    let now = Instant::now();
    active.next_deadline = now + Duration::from_secs(1);
    active.frame_poll_deadline = Some(now - Duration::from_millis(1));
    assert!(active.wake_deadline() < active.next_deadline);
    state.lock().ready = true;
    assert!(service(&mut active, now));
    assert_eq!(
        active.mailbox.snapshot().unwrap().unwrap().pixels(),
        [1, 17, 29, 255]
    );
    assert_eq!(state.lock().advances, [1]);
    assert_eq!(
        active.last_tick, anchor,
        "ordinary completion preserves the logic clock"
    );
    assert_eq!(active.wake_deadline(), active.next_deadline);
    active.close().unwrap();
}

#[test]
fn ordinary_pending_readback_does_not_suspend_a_due_logic_tick() {
    let (mut active, state) = active(false, false);
    active.advance(1, &[]).unwrap();
    let now = Instant::now();
    active.frame_poll_deadline = Some(now - Duration::from_millis(1));
    active.next_deadline = now - Duration::from_millis(1);
    assert!(service(&mut active, now));
    assert_eq!(state.lock().advances.len(), 2);
    assert_eq!(
        active.mailbox.snapshot().unwrap().unwrap().pixels(),
        [0, 0, 0, 255]
    );
    assert!(active.frame_poll_deadline.is_some());
    active.close().unwrap();
}

#[test]
fn restore_waits_for_ready_then_reanchors_once_for_tick_and_host_events() {
    for host_event in [false, true] {
        let (mut active, state) = active(true, false);
        if host_event {
            active
                .advance_host_events(&[FamilyEvent::WindowFocused { focused: false }])
                .unwrap();
        } else {
            active.advance(1, &[]).unwrap();
        }
        assert!(active.clock_reset_pending);
        let now = Instant::now();
        active.last_tick = now - Duration::from_secs(60);
        active.next_deadline = now - Duration::from_secs(59);
        active.frame_poll_deadline = Some(now - Duration::from_millis(1));
        assert!(!service(&mut active, now));
        assert_eq!(state.lock().advances.len(), 1);
        assert!(active.wake_deadline() > active.next_deadline);
        assert_eq!(
            active.mailbox.snapshot().unwrap().unwrap().pixels(),
            [0, 0, 0, 255]
        );
        state.lock().ready = true;
        let now = Instant::now();
        active.frame_poll_deadline = Some(now - Duration::from_millis(1));
        assert!(service(&mut active, now));
        let completion = state.lock().completed.unwrap();
        assert!(active.last_tick >= completion);
        assert!(!active.clock_reset_pending);
        assert_eq!(
            active.next_deadline,
            active.last_tick + Duration::from_nanos(FIXED_FRAME_NS)
        );
        let anchor = active.last_tick;
        assert!(!service(&mut active, anchor));
        assert_eq!(active.last_tick, anchor);
        assert_eq!(state.lock().advances.len(), 1);
        active.close().unwrap();
    }
}

#[test]
fn initial_frame_pending_does_not_start_vm_time_before_ready() {
    let (mut active, state) = active(false, true);
    active.capture_initial_frame().unwrap();
    assert!(active.clock_reset_pending);
    let now = Instant::now();
    active.next_deadline = now - Duration::from_secs(1);
    active.frame_poll_deadline = Some(now - Duration::from_millis(1));
    assert!(!service(&mut active, now));
    assert!(state.lock().advances.is_empty());
    assert!(active.mailbox.snapshot().unwrap().is_none());
    state.lock().ready = true;
    let now = Instant::now();
    active.frame_poll_deadline = Some(now - Duration::from_millis(1));
    assert!(service(&mut active, now));
    assert!(active.last_tick >= state.lock().completed.unwrap());
    assert_eq!(
        active.mailbox.snapshot().unwrap().unwrap().pixels(),
        [0, 17, 29, 255]
    );
    assert!(state.lock().advances.is_empty());
    active.close().unwrap();
}
