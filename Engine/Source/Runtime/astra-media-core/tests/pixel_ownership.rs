use astra_media_core::OwnedPixelBuffer;
use std::sync::Arc;

#[test]
fn shared_capture_keeps_its_allocation_until_a_mutable_update() {
    let capture: Arc<[u8]> = vec![12, 34, 56, 255].into();
    let original = Arc::clone(&capture);
    let mut pixels = OwnedPixelBuffer::from(capture);
    assert_eq!(pixels.allocation_ptr(), original.as_ptr());
    let retained_frame = pixels.clone();
    pixels.make_mut_for_update()[0] = 99;
    assert_eq!(original.as_ref(), &[12, 34, 56, 255]);
    assert_eq!(retained_frame.as_slice(), original.as_ref());
    assert_eq!(retained_frame.allocation_ptr(), original.as_ptr());
    assert_eq!(pixels.as_slice(), &[99, 34, 56, 255]);
    assert_ne!(pixels.allocation_ptr(), original.as_ptr());
    drop(original);
    assert_eq!(retained_frame.as_slice(), &[12, 34, 56, 255]);
}
