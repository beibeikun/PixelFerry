//! Read-only enumeration; no device is opened for writing until connect is requested.
use crate::{
    lg600::{self, Control},
    Connection, FramePump, PacketOutput,
};
use anyhow::{bail, ensure, Context, Result};
use pixelferry_core::*;
use sha2::{Digest, Sha256};
use std::{
    ffi::c_void,
    fs::{File, OpenOptions},
    io::Write,
    mem::size_of,
    os::windows::fs::OpenOptionsExt,
    ptr,
};
type Handle = *mut c_void;
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Guid {
    a: u32,
    b: u16,
    c: u16,
    d: [u8; 8],
}
#[repr(C)]
struct InterfaceData {
    size: u32,
    class: Guid,
    flags: u32,
    reserved: usize,
}
#[repr(C)]
struct DevInfo {
    size: u32,
    class: Guid,
    instance: u32,
    reserved: usize,
}
#[repr(C)]
struct Attributes {
    size: u32,
    vid: u16,
    pid: u16,
    version: u16,
}
#[link(name = "setupapi")]
extern "system" {
    fn SetupDiGetClassDevsW(
        class: *const Guid,
        enumerator: *const u16,
        parent: Handle,
        flags: u32,
    ) -> Handle;
    fn SetupDiEnumDeviceInterfaces(
        set: Handle,
        info: *const DevInfo,
        class: *const Guid,
        index: u32,
        data: *mut InterfaceData,
    ) -> i32;
    fn SetupDiGetDeviceInterfaceDetailW(
        set: Handle,
        interface: *const InterfaceData,
        detail: *mut c_void,
        size: u32,
        required: *mut u32,
        info: *mut DevInfo,
    ) -> i32;
    fn SetupDiGetDevicePropertyW(
        set: Handle,
        info: *const DevInfo,
        key: *const PropertyKey,
        kind: *mut u32,
        buffer: *mut u8,
        size: u32,
        required: *mut u32,
        flags: u32,
    ) -> i32;
    fn SetupDiDestroyDeviceInfoList(set: Handle) -> i32;
}
#[repr(C)]
struct PropertyKey {
    guid: Guid,
    pid: u32,
}
#[link(name = "hid")]
extern "system" {
    fn HidD_GetHidGuid(guid: *mut Guid);
    fn HidD_GetAttributes(handle: Handle, attributes: *mut Attributes) -> u8;
    fn HidD_GetSerialNumberString(handle: Handle, buffer: *mut c_void, length: u32) -> u8;
    fn HidD_GetPreparsedData(handle: Handle, data: *mut *mut c_void) -> u8;
    fn HidD_FreePreparsedData(data: *mut c_void) -> u8;
    fn HidP_GetCaps(data: *const c_void, caps: *mut u16) -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn CreateFileW(
        path: *const u16,
        access: u32,
        share: u32,
        security: *const c_void,
        creation: u32,
        flags: u32,
        template: Handle,
    ) -> Handle;
    fn CloseHandle(h: Handle) -> i32;
}
struct InfoSet(Handle);
impl Drop for InfoSet {
    fn drop(&mut self) {
        unsafe {
            SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}
struct NativeHandle(Handle);
impl Drop for NativeHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
#[derive(Debug, Clone)]
struct Endpoint {
    path: String,
    serial: Option<String>,
    container: Option<String>,
    interface: u8,
    output: u16,
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn guid_text(g: Guid) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{}",
        g.a,
        g.b,
        g.c,
        g.d[0],
        g.d[1],
        g.d[2..]
            .iter()
            .map(|n| format!("{n:02x}"))
            .collect::<String>()
    )
}
fn endpoints() -> Result<Vec<Endpoint>> {
    unsafe {
        let mut guid = Guid::default();
        HidD_GetHidGuid(&mut guid);
        let set = InfoSet(SetupDiGetClassDevsW(
            &guid,
            ptr::null(),
            ptr::null_mut(),
            0x12,
        ));
        ensure!(set.0 as isize != -1, "HID 枚举失败");
        let mut out = vec![];
        for index in 0..4096 {
            let mut interface = InterfaceData {
                size: size_of::<InterfaceData>() as u32,
                class: Guid::default(),
                flags: 0,
                reserved: 0,
            };
            if SetupDiEnumDeviceInterfaces(set.0, ptr::null(), &guid, index, &mut interface) == 0 {
                break;
            }
            let mut required = 0;
            SetupDiGetDeviceInterfaceDetailW(
                set.0,
                &interface,
                ptr::null_mut(),
                0,
                &mut required,
                ptr::null_mut(),
            );
            if !(8..=65536).contains(&required) {
                continue;
            }
            let mut detail = vec![0usize; (required as usize).div_ceil(size_of::<usize>())];
            *(detail.as_mut_ptr() as *mut u32) = 8;
            let mut info = DevInfo {
                size: size_of::<DevInfo>() as u32,
                class: Guid::default(),
                instance: 0,
                reserved: 0,
            };
            if SetupDiGetDeviceInterfaceDetailW(
                set.0,
                &interface,
                detail.as_mut_ptr().cast(),
                required,
                ptr::null_mut(),
                &mut info,
            ) == 0
            {
                continue;
            }
            let chars = std::slice::from_raw_parts(
                (detail.as_ptr() as *const u8).add(4) as *const u16,
                (required as usize - 4) / 2,
            );
            let path = String::from_utf16_lossy(
                &chars[..chars.iter().position(|n| *n == 0).unwrap_or(chars.len())],
            );
            let lower = path.to_lowercase();
            if !lower.contains("vid_1a2c&pid_1520") {
                continue;
            }
            let interface = if lower.contains("mi_00") {
                0
            } else if lower.contains("mi_01") {
                1
            } else {
                continue;
            };
            let handle = NativeHandle(CreateFileW(
                wide(&path).as_ptr(),
                0,
                3,
                ptr::null(),
                3,
                0,
                ptr::null_mut(),
            ));
            if handle.0 as isize == -1 {
                continue;
            }
            let mut attr = Attributes {
                size: size_of::<Attributes>() as u32,
                vid: 0,
                pid: 0,
                version: 0,
            };
            if HidD_GetAttributes(handle.0, &mut attr) == 0
                || attr.vid != lg600::VID
                || attr.pid != lg600::PID
            {
                continue;
            }
            let mut prep = ptr::null_mut();
            if HidD_GetPreparsedData(handle.0, &mut prep) == 0 {
                continue;
            }
            let mut caps = [0u16; 32];
            let result = HidP_GetCaps(prep, caps.as_mut_ptr());
            HidD_FreePreparsedData(prep);
            if result != 0x00110000 || caps[1] != lg600::USAGE_PAGE {
                continue;
            }
            let mut serial = [0u16; 260];
            let serial =
                if HidD_GetSerialNumberString(handle.0, serial.as_mut_ptr().cast(), 520) != 0 {
                    let s = String::from_utf16_lossy(
                        &serial[..serial.iter().position(|n| *n == 0).unwrap_or(260)],
                    );
                    if s.is_empty() {
                        None
                    } else {
                        Some(s)
                    }
                } else {
                    None
                };
            let key = PropertyKey {
                guid: Guid {
                    a: 0x8c7ed206,
                    b: 0x3f8a,
                    c: 0x4827,
                    d: [0xb3, 0xab, 0xae, 0x9e, 0x1f, 0xae, 0xfc, 0x6c],
                },
                pid: 2,
            };
            let mut container = Guid::default();
            let mut kind = 0;
            let container = if SetupDiGetDevicePropertyW(
                set.0,
                &info,
                &key,
                &mut kind,
                (&mut container as *mut Guid).cast(),
                16,
                ptr::null_mut(),
                0,
            ) != 0
                && kind == 0x0d
            {
                Some(guid_text(container))
            } else {
                None
            };
            out.push(Endpoint {
                path,
                serial,
                container,
                interface,
                output: caps[3],
            });
        }
        Ok(out)
    }
}
fn pairs() -> Result<Vec<(DeviceDescriptor, Endpoint, Endpoint)>> {
    let list = endpoints()?;
    let mut out = vec![];
    for command in list
        .iter()
        .filter(|p| p.interface == 0 && p.output as usize == lg600::COMMAND_LENGTH)
    {
        let candidates: Vec<_> = list
            .iter()
            .filter(|p| {
                p.interface == 1
                    && p.output as usize == lg600::IMAGE_LENGTH
                    && match (&command.container, &p.container) {
                        (Some(a), Some(b)) => a == b,
                        _ => command.serial.is_some() && command.serial == p.serial,
                    }
            })
            .collect();
        if candidates.len() != 1 {
            continue;
        }
        let data = candidates[0];
        let identity = command
            .container
            .as_ref()
            .or(command.serial.as_ref())
            .unwrap();
        let hash = format!("{:x}", Sha256::digest(identity.as_bytes()));
        let id = DeviceId(format!("lg600-{}", &hash[..24]));
        let duplicate_serial = list
            .iter()
            .filter(|p| p.interface == 0 && p.serial == command.serial)
            .count()
            > 1;
        out.push((
            DeviceDescriptor {
                id,
                adapter: "lg600".into(),
                name: "LG600".into(),
                serial: command.serial.clone(),
                container: command.container.clone(),
                requires_confirmation: command.container.is_none() || duplicate_serial,
                screens: vec![ScreenDescriptor {
                    id: "lcd".into(),
                    name: "冷头屏幕".into(),
                    width: 480,
                    height: 480,
                    brightness: true,
                    direction: true,
                    power: true,
                    encoding: FrameFormat::Jpeg,
                    repeat_fps: 80,
                }],
            },
            command.clone(),
            data.clone(),
        ));
    }
    Ok(out)
}
pub fn discover_lg600() -> Result<Vec<DeviceDescriptor>> {
    Ok(pairs()?.into_iter().map(|p| p.0).collect())
}
struct HidOutput(File);
impl PacketOutput for HidOutput {
    fn write(&mut self, p: &[u8]) -> Result<usize> {
        Ok(self.0.write(p)?)
    }
}
pub struct LgConnection {
    command: File,
    data: HidOutput,
    pump: FramePump,
}
impl LgConnection {
    pub fn open(device: &DeviceDescriptor) -> Result<Self> {
        let conflicts = crate::conflicts();
        ensure!(
            conflicts.is_empty(),
            "设备可能被占用，请先退出 {}",
            conflicts.join("、")
        );
        let (_, cmd, data) = pairs()?
            .into_iter()
            .find(|p| p.0.id == device.id)
            .context("LG600 已拔出或设备身份变化")?;
        let open = |p: &str| {
            OpenOptions::new()
                .write(true)
                .share_mode(3)
                .open(p)
                .with_context(|| "无法打开 HID，请检查占用")
        };
        Ok(Self {
            command: open(&cmd.path)?,
            data: HidOutput(open(&data.path)?),
            pump: FramePump::default(),
        })
    }
}
impl Connection for LgConnection {
    fn send(&mut self, screen: &ScreenId, frame: &OutputFrame<'_>) -> Result<usize> {
        ensure!(screen.0 == "lcd", "未知 LG600 屏幕");
        frame.validate()?;
        ensure!(
            frame.width == 480 && frame.height == 480 && frame.format == FrameFormat::Jpeg,
            "LG600 需要 480×480 JPEG 帧"
        );
        let before = self.pump.bytes;
        self.pump
            .send(&mut self.data, lg600::packetize(frame.bytes)?)?;
        Ok((self.pump.bytes - before) as usize)
    }
    fn controls(&mut self, screen: &ScreenId, s: &Settings) -> Result<()> {
        ensure!(screen.0 == "lcd", "未知 LG600 屏幕");
        s.validate()?;
        for (op, value) in [
            (Control::Brightness, s.brightness),
            (Control::Direction, s.direction),
            (Control::Power, if s.power { 0 } else { 1 }),
        ] {
            let p = lg600::command(op, value)?;
            if self.command.write(&p)? != p.len() {
                bail!("命令口短写")
            }
        }
        Ok(())
    }
}
