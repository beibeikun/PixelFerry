use anyhow::{ensure, Context, Result};
use pixelferry_core::*;
use pixelferry_render::DataSnapshot;
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};
pub const MAX_MESSAGE: usize = 64 * 1024 * 1024;
#[derive(Serialize, Deserialize, Clone)]
pub enum Request {
    Snapshot {
        preview: Option<(DeviceId, ScreenId)>,
    },
    Status,
    SetState(AppState),
    Reconnect(DeviceId),
    CrashSimulation(DeviceId),
    SetKey(String),
    ClearKey,
    Refresh,
    ReloadServices,
    Quit,
}
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Response {
    pub version: u32,
    pub devices: Vec<DeviceDescriptor>,
    pub statuses: Vec<DeviceStatus>,
    pub data: DataSnapshot,
    pub preview: Option<Preview>,
    pub error: Option<String>,
}
#[derive(Serialize, Deserialize, Clone)]
pub struct Preview {
    pub width: u32,
    pub height: u32,
    pub rgba: String,
    pub sequence: u64,
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    version: u32,
    request: Request,
}
pub fn write<T: Serialize>(file: &mut impl Write, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() <= MAX_MESSAGE, "IPC 消息超限");
    file.write_all(&(bytes.len() as u32).to_le_bytes())?;
    file.write_all(&bytes)?;
    file.flush()?;
    Ok(())
}
pub fn read<T: for<'de> Deserialize<'de>>(file: &mut impl Read) -> Result<T> {
    let mut length = [0; 4];
    file.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    ensure!(length > 0 && length <= MAX_MESSAGE, "IPC 消息长度无效");
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}
pub fn pipe_name(root: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    root.to_string_lossy().to_lowercase().hash(&mut h);
    format!(r"\\.\pipe\PixelFerry-{:016x}", h.finish())
}
pub fn request(root: &Path, request: Request) -> Result<Response> {
    let name = pipe_name(root);
    let mut f = {
        let mut result = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&name);
        #[cfg(windows)]
        for _ in 0..10 {
            if result.as_ref().err().and_then(|e| e.raw_os_error()) != Some(231) {
                break;
            }
            let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
            unsafe {
                windows_sys::Win32::System::Pipes::WaitNamedPipeW(wide.as_ptr(), 1000);
            };
            result = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&name);
        }
        result.context("PixelFerry 后台尚未就绪")?
    };
    write(
        &mut f,
        &Envelope {
            version: IPC_VERSION,
            request,
        },
    )?;
    let r: Response = read(&mut f)?;
    ensure!(
        r.version == IPC_VERSION,
        "后台通信版本不匹配，请完全退出后重启 PixelFerry"
    );
    if let Some(e) = &r.error {
        anyhow::bail!("{e}")
    }
    Ok(r)
}
#[cfg(windows)]
pub struct Server {
    file: File,
}
#[cfg(windows)]
impl Server {
    pub fn new(root: &Path) -> Result<Self> {
        use std::{os::windows::io::FromRawHandle, ptr};
        use windows_sys::Win32::{
            Foundation::{CloseHandle, LocalFree},
            Security::Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            },
            Security::{
                GetTokenInformation, TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
            },
            System::{
                Pipes::{
                    CreateNamedPipeW, PIPE_NOWAIT, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
                    PIPE_TYPE_BYTE,
                },
                Threading::{GetCurrentProcess, OpenProcessToken},
            },
        };
        unsafe {
            let mut token = ptr::null_mut();
            ensure!(
                OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) != 0,
                "无法读取当前用户身份"
            );
            let mut needed = 0;
            GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut needed);
            let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
            let ok = GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            );
            CloseHandle(token);
            ensure!(ok != 0, "无法读取当前用户 SID");
            let user = &*(buffer.as_ptr() as *const TOKEN_USER);
            let mut sid = ptr::null_mut();
            ensure!(
                ConvertSidToStringSidW(user.User.Sid, &mut sid) != 0,
                "SID 转换失败"
            );
            let mut len = 0;
            while *sid.add(len) != 0 {
                len += 1
            }
            let sid_string = String::from_utf16_lossy(std::slice::from_raw_parts(sid, len));
            LocalFree(sid.cast());
            let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid_string})")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let mut descriptor = ptr::null_mut();
            ensure!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut descriptor,
                    ptr::null_mut()
                ) != 0,
                "无法设置管道权限"
            );
            let security = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            };
            let name: Vec<u16> = pipe_name(root).encode_utf16().chain(Some(0)).collect();
            let handle = CreateNamedPipeW(
                name.as_ptr(),
                3 | 0x00080000,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_REJECT_REMOTE_CLIENTS | PIPE_NOWAIT,
                1,
                65536,
                65536,
                1000,
                &security,
            );
            LocalFree(descriptor);
            ensure!(handle as isize != -1, "后台已运行或管道无法创建");
            Ok(Self {
                file: File::from_raw_handle(handle),
            })
        }
    }
    pub fn poll(&mut self) -> Result<Option<Request>> {
        use std::{os::windows::io::AsRawHandle, ptr};
        use windows_sys::Win32::{
            Foundation::{GetLastError, ERROR_NO_DATA, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING},
            System::Pipes::{
                ConnectNamedPipe, DisconnectNamedPipe, PeekNamedPipe, SetNamedPipeHandleState,
                PIPE_NOWAIT, PIPE_WAIT,
            },
        };
        unsafe {
            let h = self.file.as_raw_handle();
            if ConnectNamedPipe(h, ptr::null_mut()) == 0 {
                let e = GetLastError();
                if e == ERROR_PIPE_LISTENING {
                    return Ok(None);
                }
                if e == ERROR_NO_DATA {
                    DisconnectNamedPipe(h);
                    return Ok(None);
                }
                ensure!(e == ERROR_PIPE_CONNECTED, "本地管道连接失败 {e}");
            }
            let mut available = 0;
            if PeekNamedPipe(
                h,
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                &mut available,
                ptr::null_mut(),
            ) == 0
            {
                DisconnectNamedPipe(h);
                return Ok(None);
            }
            if available < 4 {
                return Ok(None);
            }
            let mode = PIPE_WAIT;
            SetNamedPipeHandleState(h, &mode, ptr::null(), ptr::null());
            let envelope: Result<Envelope> = read(&mut self.file);
            if let Ok(e) = envelope {
                if e.version != IPC_VERSION {
                    self.respond(&Response {
                        version: IPC_VERSION,
                        error: Some("通信版本不匹配".into()),
                        ..Default::default()
                    })?;
                    return Ok(None);
                }
                return Ok(Some(e.request));
            }
            let mode = PIPE_NOWAIT;
            SetNamedPipeHandleState(h, &mode, ptr::null(), ptr::null());
            DisconnectNamedPipe(h);
            envelope.map(|e| Some(e.request))
        }
    }
    pub fn respond(&mut self, response: &Response) -> Result<()> {
        use std::{os::windows::io::AsRawHandle, ptr};
        use windows_sys::Win32::System::Pipes::{
            DisconnectNamedPipe, SetNamedPipeHandleState, PIPE_NOWAIT,
        };
        let result = write(&mut self.file, response);
        unsafe {
            let h = self.file.as_raw_handle();
            if result.is_ok() {
                windows_sys::Win32::Storage::FileSystem::FlushFileBuffers(h);
            }
            let mode = PIPE_NOWAIT;
            SetNamedPipeHandleState(h, &mode, ptr::null(), ptr::null());
            DisconnectNamedPipe(h);
        }
        result
    }
}
#[cfg(not(windows))]
pub struct Server;
#[cfg(not(windows))]
impl Server {
    pub fn new(_: &Path) -> Result<Self> {
        anyhow::bail!("后台 IPC 首版支持 Windows")
    }
    pub fn poll(&mut self) -> Result<Option<Request>> {
        Ok(None)
    }
    pub fn respond(&mut self, _: &Response) -> Result<()> {
        Ok(())
    }
}
