use super::*;

struct WindowSession(std::collections::VecDeque<Option<FamilyWindowCommand>>);
impl FamilySession for WindowSession {
    fn advance(&mut self, _: u64, _: &[FamilyEvent]) -> FamilyResult<AdvanceResponse> {
        Ok(AdvanceResponse {
            window_command: self.0.pop_front().flatten().into(),
            ..AdvanceResponse::running()
        })
    }
    fn visit_frame(&self, visitor: &mut dyn FrameVisitor) -> FamilyResult<()> {
        visitor.accept(FrameView::from_slice(
            &[0, 0, 0, 255],
            FrameInfo {
                width: 1,
                height: 1,
                stride: 4,
                logical_width: 1,
                logical_height: 1,
                format: FrameFormat::Rgba8Srgb {
                    alpha: FrameAlpha::Opaque,
                },
            },
        )?)
    }
    fn close(self: Box<Self>) -> FamilyResult<()> {
        Ok(())
    }
}

#[test]
fn window_request_persists_until_explicitly_replaced() {
    let mut active = ActiveFamilySession {
        family_id: "test".into(),
        session: Some(Box::new(WindowSession(
            [
                Some(FamilyWindowCommand::SetFullscreen(true)),
                None,
                Some(FamilyWindowCommand::SetFullscreen(false)),
            ]
            .into(),
        ))),
        audio: None,
        text: None,
        mailbox: FrameMailbox::new(),
        status: FamilyStatus::Running,
        fullscreen: false,
        title: None,
        pending_icon: None,
        last_tick: Instant::now(),
        next_deadline: Instant::now(),
    };
    active.advance(1, &[]).unwrap();
    assert!(active.fullscreen);
    active.advance(1, &[]).unwrap();
    assert!(active.fullscreen);
    active.advance(1, &[]).unwrap();
    assert!(!active.fullscreen);
    active.close().unwrap();
}

#[test]
fn title_commands_persist_clear_and_reject_invalid_updates() {
    let mut active = ActiveFamilySession {
        family_id: "test".into(),
        session: Some(Box::new(WindowSession(
            [
                Some(FamilyWindowCommand::SetTitle("公開タイトル".into())),
                None,
                Some(FamilyWindowCommand::SetTitle("bad\nprivate".into())),
                Some(FamilyWindowCommand::SetIcon(FamilyWindowIcon {
                    width: 1,
                    height: 1,
                    rgba: vec![1, 2, 3, 255].into(),
                })),
                Some(FamilyWindowCommand::SetTitle("".into())),
            ]
            .into(),
        ))),
        audio: None,
        text: None,
        mailbox: FrameMailbox::new(),
        status: FamilyStatus::Running,
        fullscreen: false,
        title: None,
        pending_icon: None,
        last_tick: Instant::now(),
        next_deadline: Instant::now(),
    };
    assert_eq!(active.title, None);
    active.advance(1, &[]).unwrap();
    assert_eq!(active.title.as_deref(), Some("公開タイトル"));
    active.advance_host_events(&[]).unwrap();
    assert_eq!(active.title.as_deref(), Some("公開タイトル"));
    let error = active.advance(1, &[]).unwrap_err();
    assert!(error.contains("ASTRA_EMU_FAMILY_WINDOW_TITLE"));
    assert!(!error.contains("private"));
    assert_eq!(active.title.as_deref(), Some("公開タイトル"));
    active.advance_host_events(&[]).unwrap();
    assert_eq!(active.title.as_deref(), Some("公開タイトル"));
    let icon = active.pending_icon.take().unwrap();
    assert_eq!(icon.rgba.as_slice(), &[1, 2, 3, 255]);
    assert!(active.pending_icon.is_none());
    active.advance(1, &[]).unwrap();
    assert_eq!(active.title.as_deref(), Some(""));
    active.close().unwrap();
    assert!(active.session.is_none());
}
