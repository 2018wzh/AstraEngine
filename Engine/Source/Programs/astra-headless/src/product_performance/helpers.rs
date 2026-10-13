use std::time::{Duration, Instant};

use astra_observability::PerfettoTraceWriter;
use astra_platform::PlatformError;

use super::*;

pub(crate) fn trace_complete_nonzero(
    trace: &mut PerfettoTraceWriter,
    domain: &'static str,
    name: &'static str,
    thread_id: u32,
    sequence: u64,
    timestamp_ns: u64,
    duration_ns: u64,
) -> Result<(), String> {
    if duration_ns == 0 {
        return Ok(());
    }
    trace
        .complete(
            domain,
            name,
            thread_id,
            Some(sequence),
            timestamp_ns,
            duration_ns,
        )
        .map_err(|error| error.to_string())
}

pub(crate) fn trace_counter_journaled(
    state: &mut RecorderState,
    domain: &'static str,
    name: &'static str,
    timestamp_ns: u64,
    value: u64,
    emit_on_change: bool,
) -> Result<(), String> {
    let key = (domain, name);
    let should_emit = should_emit_trace_counter(
        state.trace_counters.get(&key).copied(),
        state.paced_frame_count,
        value,
        emit_on_change,
    );
    if !should_emit {
        return Ok(());
    }
    state
        .trace
        .counter(domain, name, timestamp_ns, value)
        .map_err(|error| error.to_string())?;
    state.trace_counters.insert(
        key,
        TraceCounterSample {
            value,
            paced_frame: state.paced_frame_count,
        },
    );
    Ok(())
}

pub(crate) fn should_emit_trace_counter(
    previous: Option<TraceCounterSample>,
    paced_frame: u64,
    value: u64,
    emit_on_change: bool,
) -> bool {
    previous.is_none_or(|previous| {
        (emit_on_change && previous.value != value)
            || paced_frame.saturating_sub(previous.paced_frame) >= TRACE_COUNTER_HEARTBEAT_FRAMES
    })
}

pub(crate) fn frame_deadline_offset(
    frame_count: u64,
    presentation_rate_hz: u32,
) -> Result<Duration, &'static str> {
    if presentation_rate_hz == 0 {
        return Err("presentation rate must be non-zero");
    }
    let nanoseconds = u128::from(frame_count)
        .checked_mul(1_000_000_000)
        .ok_or("frame deadline offset overflowed")?
        / u128::from(presentation_rate_hz);
    let nanoseconds: u64 = nanoseconds
        .try_into()
        .map_err(|_| "frame deadline offset overflowed")?;
    Ok(Duration::from_nanos(nanoseconds))
}

pub(crate) fn next_frame_deadline(
    previous_deadline: Option<Instant>,
    now: Instant,
    presentation_rate_hz: u32,
) -> Result<Instant, &'static str> {
    let Some(previous_deadline) = previous_deadline else {
        return Ok(now);
    };
    let frame_period = frame_deadline_offset(1, presentation_rate_hz)?;
    let scheduled = previous_deadline
        .checked_add(frame_period)
        .ok_or("frame deadline overflowed")?;
    // Authored waits intentionally leave the GPU idle. Do not replay the
    // elapsed presentation slots as a catch-up burst when rendering resumes.
    Ok(scheduled.max(now))
}

pub(crate) fn pace_until(deadline: Instant) {
    const SPIN_WINDOW: Duration = Duration::from_micros(200);
    loop {
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let remaining = deadline.duration_since(now);
        if remaining > SPIN_WINDOW {
            std::thread::sleep(remaining - SPIN_WINDOW);
        } else {
            std::hint::spin_loop();
        }
    }
}

pub(crate) fn performance_error(message: &str) -> PlatformError {
    PlatformError::new(
        astra_platform::PlatformErrorCode::InvalidState,
        "headless.performance.record",
        message,
    )
}

pub(crate) fn elapsed_ns(started: Instant) -> Result<u64, String> {
    started
        .elapsed()
        .as_nanos()
        .try_into()
        .map_err(|_| "ASTRA_PERFORMANCE_TIMESTAMP_OVERFLOW".into())
}
