//! Kill subprocess descendants when their owning application process exits.
use anyhow::Result;
#[cfg(windows)]
pub struct Job(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl Job {
    pub fn attach(child: &std::process::Child) -> Result<Self> {
        use anyhow::ensure;
        use std::{mem, os::windows::io::AsRawHandle, ptr};
        use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};
        unsafe {
            let handle = CreateJobObjectW(ptr::null(), ptr::null());
            ensure!(!handle.is_null(), "无法创建媒体进程组");
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                mem::size_of_val(&info) as u32,
            ) != 0
                && AssignProcessToJobObject(handle, child.as_raw_handle()) != 0;
            if !ok {
                CloseHandle(handle);
                anyhow::bail!("无法设置子进程生命周期")
            }
            Ok(Self(handle))
        }
    }
}
#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(not(windows))]
pub struct Job;
#[cfg(not(windows))]
impl Job {
    pub fn attach(_: &std::process::Child) -> Result<Self> {
        Ok(Self)
    }
}
// Handles stay private; closing a job from another thread is supported by Windows.
#[cfg(windows)]
unsafe impl Send for Job {}
