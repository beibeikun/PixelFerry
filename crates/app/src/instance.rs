use anyhow::{ensure, Result};
#[cfg(windows)]
pub struct EditorGuard(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl Drop for EditorGuard {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(windows)]
pub fn acquire(root: &std::path::Path) -> Result<Option<EditorGuard>> {
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS},
        System::Threading::CreateMutexW,
        UI::WindowsAndMessaging::{
            FindWindowW, SetForegroundWindow, ShowWindow, ShowWindowAsync, SW_RESTORE, SW_SHOW,
        },
    };
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let name = wide(&format!(
        "Local\\{}",
        crate::ipc::pipe_name(root).replace('\\', "-")
    ));
    unsafe {
        let mutex = CreateMutexW(ptr::null(), 0, name.as_ptr());
        ensure!(!mutex.is_null(), "无法创建 PixelFerry 编辑器锁");
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(mutex);
            let window = FindWindowW(ptr::null(), wide("PixelFerry · 像素渡轮").as_ptr());
            if !window.is_null() {
                ShowWindow(window, SW_SHOW);
                // A launcher can pass STARTUPINFO/SW_HIDE, which overrides the
                // first ShowWindow call made by this short-lived process.
                ShowWindowAsync(window, SW_RESTORE);
                ShowWindowAsync(window, SW_SHOW);
                SetForegroundWindow(window);
            }
            return Ok(None);
        }
        #[link(name = "shell32")]
        extern "system" {
            fn SetCurrentProcessExplicitAppUserModelID(id: *const u16) -> i32;
        }
        SetCurrentProcessExplicitAppUserModelID(wide("pixelferry").as_ptr());
        Ok(Some(EditorGuard(mutex)))
    }
}
#[cfg(not(windows))]
pub struct EditorGuard;
#[cfg(not(windows))]
pub fn acquire(_: &std::path::Path) -> Result<Option<EditorGuard>> {
    Ok(Some(EditorGuard))
}
