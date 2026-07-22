#[cfg(windows)]
mod platform {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::Threading::{
        AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, AvSetMmThreadPriority,
        GetCurrentProcess, GetCurrentThread, GetPriorityClass, GetThreadPriority, SetPriorityClass,
        SetThreadPriority, AVRT_PRIORITY_HIGH, HIGH_PRIORITY_CLASS, THREAD_PRIORITY_HIGHEST,
    };

    const THREAD_PRIORITY_ERROR_RETURN: i32 = i32::MAX;

    pub struct PerformanceSchedulingGuard {
        process_priority: u32,
        thread_priority: i32,
        mmcss_handle: Option<HANDLE>,
        restored: bool,
    }

    impl PerformanceSchedulingGuard {
        pub fn activate() -> Result<Self, String> {
            Self::activate_with_policy(THREAD_PRIORITY_HIGHEST, Some(AVRT_PRIORITY_HIGH))
        }

        pub fn activate_coordinator() -> Result<Self, String> {
            Self::activate_with_policy(THREAD_PRIORITY_HIGHEST, Some(AVRT_PRIORITY_HIGH))
        }

        fn activate_with_policy(
            thread_priority_target: i32,
            mmcss_priority: Option<i32>,
        ) -> Result<Self, String> {
            // SAFETY: current-process/current-thread pseudo handles remain valid for the call
            // and must not be closed.
            let process = unsafe { GetCurrentProcess() };
            let process_priority = unsafe { GetPriorityClass(process) };
            if process_priority == 0 {
                return Err(last_error(
                    "ASTRA_PERFORMANCE_PROCESS_PRIORITY_QUERY_FAILED",
                ));
            }
            if unsafe { SetPriorityClass(process, HIGH_PRIORITY_CLASS) } == 0 {
                return Err(last_error("ASTRA_PERFORMANCE_PROCESS_PRIORITY_SET_FAILED"));
            }

            let thread = unsafe { GetCurrentThread() };
            let thread_priority = unsafe { GetThreadPriority(thread) };
            if thread_priority == THREAD_PRIORITY_ERROR_RETURN {
                unsafe { SetPriorityClass(process, process_priority) };
                return Err(last_error("ASTRA_PERFORMANCE_THREAD_PRIORITY_QUERY_FAILED"));
            }
            if unsafe { SetThreadPriority(thread, thread_priority_target) } == 0 {
                unsafe { SetPriorityClass(process, process_priority) };
                return Err(last_error("ASTRA_PERFORMANCE_THREAD_PRIORITY_SET_FAILED"));
            }

            let mmcss_handle = if let Some(mmcss_priority) = mmcss_priority {
                let mut task_index = 0;
                let task_name = "Games\0".encode_utf16().collect::<Vec<_>>();
                let handle =
                    unsafe { AvSetMmThreadCharacteristicsW(task_name.as_ptr(), &mut task_index) };
                if handle.is_null() {
                    unsafe {
                        SetThreadPriority(thread, thread_priority);
                        SetPriorityClass(process, process_priority);
                    }
                    return Err(last_error("ASTRA_PERFORMANCE_MMCSS_REGISTER_FAILED"));
                }
                if unsafe { AvSetMmThreadPriority(handle, mmcss_priority) } == 0 {
                    unsafe {
                        AvRevertMmThreadCharacteristics(handle);
                        SetThreadPriority(thread, thread_priority);
                        SetPriorityClass(process, process_priority);
                    }
                    return Err(last_error("ASTRA_PERFORMANCE_MMCSS_PRIORITY_FAILED"));
                }
                Some(handle)
            } else {
                None
            };

            Ok(Self {
                process_priority,
                thread_priority,
                mmcss_handle,
                restored: false,
            })
        }

        pub fn restore(mut self) -> Result<(), String> {
            self.restore_inner()?;
            self.restored = true;
            Ok(())
        }

        fn restore_inner(&self) -> Result<(), String> {
            if let Some(handle) = self.mmcss_handle {
                if unsafe { AvRevertMmThreadCharacteristics(handle) } == 0 {
                    return Err(last_error("ASTRA_PERFORMANCE_MMCSS_RESTORE_FAILED"));
                }
            }
            if unsafe { SetThreadPriority(GetCurrentThread(), self.thread_priority) } == 0 {
                return Err(last_error(
                    "ASTRA_PERFORMANCE_THREAD_PRIORITY_RESTORE_FAILED",
                ));
            }
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

        pub fn activate_coordinator() -> Result<Self, String> {
            Err("ASTRA_PERFORMANCE_SCHEDULING_NOT_IMPLEMENTED".into())
        }

        pub fn restore(self) -> Result<(), String> {
            Ok(())
        }
    }
}

pub use platform::PerformanceSchedulingGuard;
