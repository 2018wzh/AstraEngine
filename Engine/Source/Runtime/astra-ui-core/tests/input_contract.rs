use astra_ui_core::{UiInsets, UiViewport, ValidateUi, MAX_UI_VIEWPORT_DIMENSION};

fn viewport(width: u32, height: u32) -> UiViewport {
    UiViewport {
        physical_width: width,
        physical_height: height,
        scale_factor: 1.0,
        font_scale: 1.0,
        safe_area_points: UiInsets {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        },
    }
}

#[astra_headless_test::test]
fn viewport_dimensions_are_hard_bounded() {
    viewport(MAX_UI_VIEWPORT_DIMENSION, MAX_UI_VIEWPORT_DIMENSION)
        .validate()
        .unwrap();
    let error = viewport(MAX_UI_VIEWPORT_DIMENSION + 1, 720)
        .validate()
        .unwrap_err();
    assert_eq!(error.code(), "ASTRA_UI_VIEWPORT_LIMIT");
}
