use super::*;
use abi_stable::std_types::ROption::{RNone, RSome};

fn request() -> ModalRequest {
    ModalRequest {
        caption: "原始标题".into(),
        message: "提示".into(),
        buttons: vec!["是".into(), "否".into()].into(),
        initial_text: RNone,
    }
}

#[test]
fn byte_limits_accept_exact_boundary_and_reject_utf8_overflow_nul_and_button_overflow() {
    let mut value = request();
    value.message = "é".repeat(MAX_MODAL_TEXT_BYTES / 2).into();
    value.buttons = vec!["a".repeat(MAX_MODAL_BUTTON_BYTES).into(); MAX_MODAL_BUTTONS].into();
    value.validate().unwrap();
    value.message.push_str("é");
    assert!(value.validate().is_err());
    value.message = "提示\0尾部".into();
    assert!(value.validate().is_err());
    value.message = "提示".into();
    value.buttons.push("超额".into());
    assert!(value.validate().is_err());
    value.buttons = vec!["é".repeat(MAX_MODAL_BUTTON_BYTES / 2 + 1).into()].into();
    assert!(value.validate().is_err());
    value.buttons = Vec::<RString>::new().into();
    assert!(value.validate().is_err());
}

#[test]
fn response_requires_original_button_and_matching_input_kind_but_cancellation_has_no_default() {
    let mut value = request();
    ModalResponse::Cancelled
        .validate_for_request(&value)
        .unwrap();
    ModalResponse::Submitted {
        button_index: 1,
        text: RNone,
    }
    .validate_for_request(&value)
    .unwrap();
    assert!(ModalResponse::Submitted {
        button_index: 2,
        text: RNone
    }
    .validate_for_request(&value)
    .is_err());
    assert!(ModalResponse::Submitted {
        button_index: 0,
        text: RSome("".into())
    }
    .validate_for_request(&value)
    .is_err());
    value.initial_text = RSome("".into());
    assert!(ModalResponse::Submitted {
        button_index: 0,
        text: RNone
    }
    .validate_for_request(&value)
    .is_err());
    ModalResponse::Submitted {
        button_index: 0,
        text: RSome("".into()),
    }
    .validate_for_request(&value)
    .unwrap();
    assert!(ModalResponse::Submitted {
        button_index: 0,
        text: RSome("a".repeat(MAX_MODAL_TEXT_BYTES + 1).into())
    }
    .validate_for_request(&value)
    .is_err());
}
