#[cfg(windows)]
mod platform {
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentThread, GetPriorityClass, GetThreadPriority, SetPriorityClass,
        SetThreadPriority, HIGH_PRIORITY_CLASS, THREAD_PRIORITY_HIGHEST,
    };

    const THREAD_PRIORITY_ERROR_RETURN: i32 = i32::MAX;

    pub struct PerformanceSchedulingGuard {
        process_priority: u32,
        thread_priority: i32,
        restored: bool,
    }

    impl PerformanceSchedulingGuard {
        pub fn activate() -> Result<Self, String> {
            // SAFETY: pseudo handles returned for the current process and thread remain valid
            // for the duration of the call and must not be closed.
            let process = unsafe { GetCurrentProcess() };
            // SAFETY: `process` is the valid current-process pseudo handle.
            let process_priority = unsafe { GetPriorityClass(process) };
            if process_priority == 0 {
                return Err(last_error(
                    "ASTRA_PERFORMANCE_PROCESS_PRIORITY_QUERY_FAILED",
                ));
            }
            // SAFETY: the API only changes the scheduling class of the current process.
            if unsafe { SetPriorityClass(process, HIGH_PRIORITY_CLASS) } == 0 {
                return Err(last_error("ASTRA_PERFORMANCE_PROCESS_PRIORITY_SET_FAILED"));
            }

            // SAFETY: pseudo handles returned for the current thread remain valid for the
            // duration of the call and must not be closed.
            let thread = unsafe { GetCurrentThread() };
            // SAFETY: `thread` is the valid current-thread pseudo handle.
            let thread_priority = unsafe { GetThreadPriority(thread) };
            if thread_priority == THREAD_PRIORITY_ERROR_RETURN {
                // SAFETY: restore the process setting before returning the activation error.
                unsafe { SetPriorityClass(process, process_priority) };
                return Err(last_error("ASTRA_PERFORMANCE_THREAD_PRIORITY_QUERY_FAILED"));
            }
            // SAFETY: the API only changes the scheduling priority of the current thread.
            if unsafe { SetThreadPriority(thread, THREAD_PRIORITY_HIGHEST) } == 0 {
                // SAFETY: restore the process setting before returning the activation error.
                unsafe { SetPriorityClass(process, process_priority) };
                return Err(last_error("ASTRA_PERFORMANCE_THREAD_PRIORITY_SET_FAILED"));
            }

            Ok(Self {
                process_priority,
                thread_priority,
                restored: false,
            })
        }

        pub fn restore(mut self) -> Result<(), String> {
            self.restore_inner()?;
            self.restored = true;
            Ok(())
        }

        fn restore_inner(&self) -> Result<(), String> {
            // SAFETY: both APIs receive current-process/current-thread pseudo handles and the
            // priority values returned by the corresponding query APIs during activation.
            if unsafe { SetThreadPriority(GetCurrentThread(), self.thread_priority) } == 0 {
                return Err(last_error(
                    "ASTRA_PERFORMANCE_THREAD_PRIORITY_RESTORE_FAILED",
                ));
            }
            // SAFETY: see above.
            if unsafe { SetPriorityClass(GetCurrentProcess(), self.process_priority) } == 0 {
                return Err(last_error(
                    "ASTRA_PERFORMANCE_PROCESS_PRIORITY_RESTORE_FAILED",
                ));
            }
            Ok(())
        }
    }

    impl Drop for PerformanceSchedulingGuard {
        fn drop(&mut self) {
            if !self.restored {
                let _ = self.restore_inner();
            }
        }
    }

    fn last_error(code: &str) -> String {
        format!("{code}: {}", std::io::Error::last_os_error())
    }
}

#[cfg(not(windows))]
mod platform {
    pub struct PerformanceSchedulingGuard;

    impl PerformanceSchedulingGuard {
        pub fn activate() -> Result<Self, String> {
            Err("ASTRA_PERFORMANCE_SCHEDULING_NOT_IMPLEMENTED".into())
        }

        pub fn restore(self) -> Result<(), String> {
            Ok(())
        }
    }
}

pub use platform::PerformanceSchedulingGuard;
