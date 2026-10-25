use anyhow::{ensure, Result};
use std::sync::atomic::{AtomicBool, Ordering};
pub static SUSPENDED: AtomicBool = AtomicBool::new(false);
#[cfg(windows)]
pub struct PowerSubscription(*mut std::ffi::c_void);
#[cfg(windows)]
impl PowerSubscription {
    pub fn new() -> Result<Self> {
        use std::ptr;
        use windows_sys::Win32::System::Power::{
            PowerRegisterSuspendResumeNotification, DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS,
        };
        unsafe extern "system" fn callback(
            _: *const std::ffi::c_void,
            event: u32,
            _: *const std::ffi::c_void,
        ) -> u32 {
            match event {
                4 => SUSPENDED.store(true, Ordering::Relaxed),
                7 | 18 => SUSPENDED.store(false, Ordering::Relaxed),
                _ => {}
            }
            0
        }
        let parameters = DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
            Callback: Some(callback),
            Context: ptr::null_mut(),
        };
        let mut handle = ptr::null_mut();
        let result = unsafe {
            PowerRegisterSuspendResumeNotification(
                2,
                (&parameters as *const DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS)
                    .cast_mut()
                    .cast(),
                &mut handle,
            )
        };
        ensure!(result == 0, "无法注册睡眠通知：{result}");
        Ok(Self(handle))
    }
}
#[cfg(windows)]
impl Drop for PowerSubscription {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::System::Power::PowerUnregisterSuspendResumeNotification(
                self.0 as isize,
            );
        }
    }
}
#[cfg(not(windows))]
pub struct PowerSubscription;
#[cfg(not(windows))]
impl PowerSubscription {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }
}
pub fn autostart(enable: bool) -> Result<()> {
    #[cfg(windows)]
    unsafe {
        use std::ptr;
        use windows_sys::Win32::System::Registry::*;
        let w = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let mut key = ptr::null_mut();
        let result = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w("Software\\Microsoft\\Windows\\CurrentVersion\\Run").as_ptr(),
            0,
            ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            ptr::null(),
            &mut key,
            ptr::null_mut(),
        );
        ensure!(result == 0, "无法修改登录启动设置");
        let name = w("PixelFerry");
        let result = if enable {
            let exe = std::env::current_exe()?;
            let command = w(&format!("\"{}\" --minimized", exe.display()));
            RegSetValueExW(
                key,
                name.as_ptr(),
                0,
                REG_SZ,
                command.as_ptr().cast(),
                (command.len() * 2) as u32,
            )
        } else {
            let r = RegDeleteValueW(key, name.as_ptr());
            if r == 2 {
                0
            } else {
                r
            }
        };
        RegCloseKey(key);
        ensure!(result == 0, "登录启动设置保存失败：{result}");
    }
    Ok(())
}
