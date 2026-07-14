use astra_platform::{PlatformErrorCode, WindowHandle};
use astra_platform_common::{OrderedCompletionQueue, ResourceTable};

#[test]
fn resource_table_rejects_stale_handles_and_reports_leaks() {
    let mut table = ResourceTable::<String, WindowHandle>::new("window");
    let first = table.insert("first".to_string()).unwrap();
    assert_eq!(table.get(first).unwrap(), "first");
    assert_eq!(table.remove(first).unwrap(), "first");
    assert_eq!(
        table.get(first).unwrap_err().code,
        PlatformErrorCode::StaleHandle
    );

    let second = table.insert("second".to_string()).unwrap();
    assert_eq!(second.parts().0, first.parts().0);
    assert!(second.parts().1 > first.parts().1);
    let error = table.ensure_empty().unwrap_err();
    assert_eq!(error.code, PlatformErrorCode::ResourceLeak);
    assert_eq!(
        error.fields.get("resource_count").map(String::as_str),
        Some("1")
    );
    table.remove(second).unwrap();
    table.ensure_empty().unwrap();
}

#[test]
fn ordered_completion_queue_only_drains_contiguous_sequences() {
    let mut queue = OrderedCompletionQueue::new(10, 2);
    queue.push(11, "second").unwrap();
    assert!(queue.drain_ready().is_empty());
    queue.push(10, "first").unwrap();
    assert_eq!(queue.drain_ready(), vec![(10, "first"), (11, "second")]);
    assert_eq!(
        queue.push(11, "duplicate").unwrap_err().code,
        PlatformErrorCode::InvalidState
    );
    queue.push(13, "fourth").unwrap();
    assert_eq!(
        queue.push(14, "overflow").unwrap_err().code,
        PlatformErrorCode::QueueOverflow
    );
}
