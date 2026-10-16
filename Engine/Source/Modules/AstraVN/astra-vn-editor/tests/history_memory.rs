use astra_vn_editor::{AstraSource, AuthoringWorkspace, DocumentEdits, EditBatch, TextEdit};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! { static LIVE_BYTES: Cell<isize> = const { Cell::new(0) }; }
struct AllocationMeter;
unsafe impl GlobalAlloc for AllocationMeter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            let _ = LIVE_BYTES.try_with(|live| live.set(live.get() + layout.size() as isize));
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        let _ = LIVE_BYTES.try_with(|live| live.set(live.get() - layout.size() as isize));
        unsafe { System.dealloc(pointer, layout) };
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let next = unsafe { System.realloc(pointer, layout, size) };
        if !next.is_null() {
            let _ = LIVE_BYTES
                .try_with(|live| live.set(live.get() + size as isize - layout.size() as isize));
        }
        next
    }
}
#[global_allocator]
static ALLOCATOR: AllocationMeter = AllocationMeter;

#[test]
fn repeated_small_changes_do_not_retain_whole_document_versions() {
    let mut workspace = AuthoringWorkspace::default();
    workspace
        .open(AstraSource::story("large.astra", "a".repeat(1024 * 1024)))
        .unwrap();
    let before = LIVE_BYTES.with(Cell::get);
    // Editor text widgets may report complete replacement strings. History must
    // normalize those into the changed UTF-8 span rather than retain each copy.
    for index in 1..=128 {
        let document = workspace.document("large.astra").unwrap();
        let mut text = document.text.clone();
        text.replace_range(0..1, &char::from(b'a' + index % 26).to_string());
        workspace
            .apply(EditBatch {
                documents: vec![DocumentEdits {
                    path: document.path.clone(),
                    version: document.version,
                    edits: vec![TextEdit {
                        start: 0,
                        end: document.text.len(),
                        replacement: text,
                    }],
                }],
            })
            .unwrap();
    }
    let retained_growth = LIVE_BYTES.with(Cell::get) - before;
    assert!(
        retained_growth < 4 * 1024 * 1024,
        "undo retained {retained_growth} bytes for one-byte edits"
    );
    for _ in 0..100 {
        workspace.undo().unwrap();
    }
    assert_eq!(
        workspace.document("large.astra").unwrap().text.as_bytes()[0],
        b'a' + 28 % 26
    );
    for _ in 0..100 {
        workspace.redo().unwrap();
    }
    assert_eq!(
        workspace.document("large.astra").unwrap().text.as_bytes()[0],
        b'a' + 128 % 26
    );
}
