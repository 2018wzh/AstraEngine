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

#[cfg(not(windows))]
pub fn sample_process_memory() -> Result<ProcessMemorySample, ProcessMemoryError> {
    Err(ProcessMemoryError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[astra_headless_test::test]
    fn samples_nonzero_working_set_and_private_bytes() {
        let sample = sample_process_memory().unwrap();
        assert!(sample.working_set_bytes > 0);
        assert!(sample.private_bytes > 0);
    }
}
