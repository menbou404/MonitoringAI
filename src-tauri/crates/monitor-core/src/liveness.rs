use chrono::{DateTime, Utc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessInfo {
    pub created_at: Option<DateTime<Utc>>,
    pub executable_name: Option<String>,
}

pub trait ProcessProbe: Send + Sync {
    /// `None` means the process is gone. Unknown details remain `None` in ProcessInfo.
    fn probe(&self, pid: u32) -> Option<ProcessInfo>;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

pub struct SystemProcessProbe;

#[cfg(not(windows))]
impl ProcessProbe for SystemProcessProbe {
    fn probe(&self, _pid: u32) -> Option<ProcessInfo> {
        Some(ProcessInfo {
            created_at: None,
            executable_name: None,
        })
    }
}

#[cfg(windows)]
impl ProcessProbe for SystemProcessProbe {
    fn probe(&self, pid: u32) -> Option<ProcessInfo> {
        probe_windows(pid)
    }
}

#[cfg(windows)]
fn probe_windows(pid: u32) -> Option<ProcessInfo> {
    use windows::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::Threading::{
            GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        },
    };
    use windows::core::PWSTR;

    // SAFETY: All pointers refer to initialized, writable local buffers. The handle is
    // closed before returning, including when an intermediate query fails.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let result = (|| {
            let mut exit_code = 0;
            GetExitCodeProcess(handle, &mut exit_code).ok()?;
            if exit_code != 259 {
                return None;
            } // STILL_ACTIVE
            let mut created = FILETIME::default();
            let mut exited = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user).ok()?;
            let ticks =
                (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
            let unix_ticks = ticks.checked_sub(116_444_736_000_000_000)?;
            let created_at = DateTime::from_timestamp(
                (unix_ticks / 10_000_000).try_into().ok()?,
                ((unix_ticks % 10_000_000) * 100).try_into().ok()?,
            );
            let mut path = vec![0u16; 32768];
            let mut size = path.len() as u32;
            QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut size,
            )
            .ok()?;
            let path = String::from_utf16_lossy(&path[..size as usize]);
            let executable_name = std::path::Path::new(&path)
                .file_name()?
                .to_string_lossy()
                .into_owned();
            Some(ProcessInfo {
                created_at,
                executable_name: Some(executable_name),
            })
        })();
        let _ = CloseHandle(handle);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_is_utc() {
        assert!(SystemClock.now() <= Utc::now());
    }
}
