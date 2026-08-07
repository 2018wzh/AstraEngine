use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessMemorySample {
    pub working_set_bytes: u64,
    pub private_bytes: u64,
}

#[derive(Debug, Error)]
pub enum ProcessMemoryError {
    #[error("ASTRA_PROCESS_MEMORY_UNSUPPORTED: process memory sampling is not implemented")]
    Unsupported,
    #[error("ASTRA_PROCESS_MEMORY_QUERY_FAILED: operating system query failed")]
    QueryFailed,
}

#[cfg(windows)]
pub fn sample_process_memory() -> Result<ProcessMemorySample, ProcessMemoryError> {
    use windows::Win32::System::{
        ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
        Threading::GetCurrentProcess,
    };

    let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
    // SAFETY: the current-process pseudo handle is valid and the buffer uses the
    // exact Win32 structure size declared for this query.
    let result = unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            std::ptr::from_mut(&mut counters).cast(),
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        )
    };
    if !result.as_bool() {
        return Err(ProcessMemoryError::QueryFailed);
    }
    Ok(ProcessMemorySample {
        working_set_bytes: counters.WorkingSetSize as u64,
        private_bytes: counters.PrivateUsage as u64,
    })
}

#[cfg(windows)]
pub fn sample_process_memory_by_pid(
    process_id: u32,
) -> Result<ProcessMemorySample, ProcessMemoryError> {
    use windows::Win32::{
        Foundation::CloseHandle,
        System::{
            ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
            Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        },
    };

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }
        .map_err(|_| ProcessMemoryError::QueryFailed)?;
    let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
    let query = unsafe {
        K32GetProcessMemoryInfo(
            process,
            std::ptr::from_mut(&mut counters).cast(),
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        )
    };
    let close = unsafe { CloseHandle(process) };
    if !query.as_bool() || close.is_err() {
        return Err(ProcessMemoryError::QueryFailed);
    }
    Ok(ProcessMemorySample {
        working_set_bytes: counters.WorkingSetSize as u64,
        private_bytes: counters.PrivateUsage as u64,
    })
}

#[cfg(windows)]
pub fn sample_process_cpu_time_us_by_pid(process_id: u32) -> Result<u64, ProcessMemoryError> {
    use windows::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }
        .map_err(|_| ProcessMemoryError::QueryFailed)?;
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let query =
        unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) };
    let close = unsafe { CloseHandle(process) };
    if query.is_err() || close.is_err() {
        return Err(ProcessMemoryError::QueryFailed);
    }
    let ticks = file_time_ticks(kernel)
        .checked_add(file_time_ticks(user))
        .ok_or(ProcessMemoryError::QueryFailed)?;
    Ok(ticks / 10)
}

#[cfg(windows)]
fn file_time_ticks(value: windows::Win32::Foundation::FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

#[cfg(target_os = "linux")]
fn parse_kib(value: &str) -> Option<u64> {
    value
        .split_whitespace()
        .next()
        .and_then(|bytes| bytes.parse::<u64>().ok())
        .and_then(|kib| kib.checked_mul(1024))
}

#[cfg(target_os = "linux")]
fn read_status_memory(pid: Option<u32>) -> Result<ProcessMemorySample, ProcessMemoryError> {
    let path = match pid {
        Some(process_id) => format!("/proc/{process_id}/status"),
        None => "/proc/self/status".to_string(),
    };
    let status = std::fs::read_to_string(path).map_err(|_| ProcessMemoryError::QueryFailed)?;
    let mut working_set_bytes = None;
    let mut private_bytes = None;
    for line in status.lines() {
        if working_set_bytes.is_none() {
            if let Some(value) = line.strip_prefix("VmRSS:") {
                working_set_bytes = parse_kib(value);
            }
        }
        if private_bytes.is_none() {
            if let Some(value) = line.strip_prefix("RssAnon:") {
                private_bytes = parse_kib(value);
            }
        }
        if working_set_bytes.is_some() && private_bytes.is_some() {
            break;
        }
    }
    Ok(ProcessMemorySample {
        working_set_bytes: working_set_bytes.ok_or(ProcessMemoryError::QueryFailed)?,
        private_bytes: private_bytes.ok_or(ProcessMemoryError::QueryFailed)?,
    })
}

#[cfg(target_os = "linux")]
pub fn sample_process_memory() -> Result<ProcessMemorySample, ProcessMemoryError> {
    read_status_memory(None)
}

#[cfg(target_os = "linux")]
pub fn sample_process_memory_by_pid(
    process_id: u32,
) -> Result<ProcessMemorySample, ProcessMemoryError> {
    read_status_memory(Some(process_id))
}

#[cfg(target_os = "linux")]
pub fn sample_process_cpu_time_us_by_pid(process_id: u32) -> Result<u64, ProcessMemoryError> {
    // `stat` fields after the parenthesised command name: utime is field 14 and
    // stime field 15 in proc(5) 1-based numbering, i.e. indexes 11 and 12 of the
    // remainder once the trailing `)` has split the command token off.
    let stat = std::fs::read_to_string(format!("/proc/{process_id}/stat"))
        .map_err(|_| ProcessMemoryError::QueryFailed)?;
    let Some(tail) = stat.rfind(')') else {
        return Err(ProcessMemoryError::QueryFailed);
    };
    let fields: Vec<&str> = stat[tail + 1..].split_whitespace().collect();
    let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks_per_second <= 0 {
        return Err(ProcessMemoryError::QueryFailed);
    }
    let utime = fields
        .get(11)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(ProcessMemoryError::QueryFailed)?;
    let stime = fields
        .get(12)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(ProcessMemoryError::QueryFailed)?;
    let ticks = utime
        .checked_add(stime)
        .ok_or(ProcessMemoryError::QueryFailed)?;
    // `sysconf` returns a positive `c_long` here; the guard above rejects
    // zero and negative values, so the cast is lossless.
    let ticks_per_second = ticks_per_second as u64;
    let micros_per_tick = 1_000_000 / ticks_per_second;
    ticks
        .checked_mul(micros_per_tick)
        .ok_or(ProcessMemoryError::QueryFailed)
}

#[cfg(all(not(windows), not(target_os = "linux")))]
pub fn sample_process_memory() -> Result<ProcessMemorySample, ProcessMemoryError> {
    Err(ProcessMemoryError::Unsupported)
}

#[cfg(all(not(windows), not(target_os = "linux")))]
pub fn sample_process_memory_by_pid(
    _process_id: u32,
) -> Result<ProcessMemorySample, ProcessMemoryError> {
    Err(ProcessMemoryError::Unsupported)
}

#[cfg(all(not(windows), not(target_os = "linux")))]
pub fn sample_process_cpu_time_us_by_pid(_process_id: u32) -> Result<u64, ProcessMemoryError> {
    Err(ProcessMemoryError::Unsupported)
}

#[cfg(test)]
mod tests {
    #[cfg(any(windows, target_os = "linux"))]
    use super::*;

    #[cfg(any(windows, target_os = "linux"))]
    #[astra_headless_test::test]
    fn samples_nonzero_working_set_and_private_bytes() {
        let sample = sample_process_memory().unwrap();
        assert!(sample.working_set_bytes > 0);
        assert!(sample.private_bytes > 0);
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[astra_headless_test::test]
    fn samples_nonzero_memory_by_process_id() {
        let sample = sample_process_memory_by_pid(std::process::id()).unwrap();
        assert!(sample.working_set_bytes > 0);
        assert!(sample.private_bytes > 0);
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[astra_headless_test::test]
    fn samples_process_cpu_time_by_process_id() {
        let before = sample_process_cpu_time_us_by_pid(std::process::id()).unwrap();
        let mut accumulator = 0_u64;
        for value in 0..1_000_000_u64 {
            accumulator = accumulator.wrapping_add(value.rotate_left(7));
        }
        std::hint::black_box(accumulator);
        let after = sample_process_cpu_time_us_by_pid(std::process::id()).unwrap();
        assert!(after >= before);
    }
}
