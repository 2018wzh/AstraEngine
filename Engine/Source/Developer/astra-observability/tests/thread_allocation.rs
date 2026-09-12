use astra_observability::{thread_allocation_snapshot, TrackingAllocator};

#[global_allocator]
static GLOBAL_ALLOCATOR: TrackingAllocator = TrackingAllocator::new();

#[test]
fn thread_snapshot_excludes_allocations_owned_by_another_thread() {
    let before = thread_allocation_snapshot();
    let worker = std::thread::spawn(|| {
        let worker_before = thread_allocation_snapshot();
        let bytes = vec![0_u8; 1024 * 1024];
        let worker_after = thread_allocation_snapshot();
        assert!(worker_after.allocated_bytes - worker_before.allocated_bytes >= bytes.len() as u64);
        std::hint::black_box(bytes);
    });
    worker.join().unwrap();
    let after = thread_allocation_snapshot();
    assert!(after.allocated_bytes - before.allocated_bytes < 1024 * 1024);
}
