use anyhow::{ensure, Result};
use pixelferry_core::SensorSample;
use std::collections::BTreeMap;
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn text(b: &[u8], at: usize) -> String {
    let bytes = &b[at..at + 260];
    let end = bytes.iter().position(|n| *n == 0).unwrap_or(260);
    #[cfg(windows)]
    {
        use windows_sys::Win32::Globalization::MultiByteToWideChar;
        let n = unsafe {
            MultiByteToWideChar(0, 0, bytes.as_ptr(), end as i32, std::ptr::null_mut(), 0)
        };
        if n > 0 {
            let mut out = vec![0u16; n as usize];
            unsafe {
                MultiByteToWideChar(0, 0, bytes.as_ptr(), end as i32, out.as_mut_ptr(), n);
            };
            return String::from_utf16_lossy(&out);
        }
    }
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}
pub fn parse(bytes: &[u8], now: i64) -> Result<Vec<SensorSample>> {
    ensure!(bytes.len() >= 32, "MAHM 头不完整");
    ensure!(
        u32_at(bytes, 0) == 0x4d41484d && u32_at(bytes, 4) >> 16 == 2,
        "不支持的 MAHM 版本"
    );
    let (header, count, stride, gpu_count, gpu_stride) = (
        u32_at(bytes, 8) as usize,
        u32_at(bytes, 12) as usize,
        u32_at(bytes, 16) as usize,
        u32_at(bytes, 24) as usize,
        u32_at(bytes, 28) as usize,
    );
    ensure!(
        (32..=4096).contains(&header)
            && count <= 4096
            && (1324..=65536).contains(&stride)
            && gpu_count <= 64
            && (1304..=65536).contains(&gpu_stride),
        "MAHM 布局无效"
    );
    let required = header + count * stride + gpu_count * gpu_stride;
    ensure!(required <= bytes.len(), "MAHM 布局越界");
    let timestamp = (u32_at(bytes, 20) as i32 as i64) * 1000;
    let mut occurrences = BTreeMap::<String, u32>::new();
    let mut out = vec![];
    for i in 0..count {
        let b = &bytes[header + i * stride..header + i * stride + 1324];
        let value = f32::from_le_bytes(b[1300..1304].try_into().unwrap());
        let source = u32_at(b, 1320);
        let gpu = u32_at(b, 1316);
        let name = text(b, 0);
        let mut unit = text(b, 260);
        if (source <= 3 || source == 0x80 || (0xb0..=0xd3).contains(&source))
            && name.to_lowercase().contains("temperature")
        {
            unit = "°C".into();
        }
        let key = format!("{source}:{gpu}:{name}");
        let occurrence = occurrences.entry(key.clone()).or_default();
        let id = format!("afterburner:{key}:{occurrence}");
        *occurrence += 1;
        let valid = value.is_finite() && value != f32::MAX;
        let device = if name.starts_with("GPU") && (gpu as usize) < gpu_count {
            text(
                bytes,
                header + count * stride + gpu as usize * gpu_stride + 520,
            )
        } else {
            String::new()
        };
        out.push(SensorSample {
            id,
            name,
            unit,
            value: valid.then_some(value as f64),
            timestamp,
            status: if !valid {
                "unavailable"
            } else if now - timestamp > 5000 {
                "stale"
            } else {
                "ok"
            }
            .into(),
            source_id: source,
            gpu_index: gpu,
            device,
        });
    }
    Ok(out)
}
#[cfg(windows)]
pub fn read() -> Result<Vec<SensorSample>> {
    use std::{ffi::c_void, mem::size_of};
    type Handle = *mut c_void;
    #[repr(C)]
    struct MemoryInfo {
        base: *mut c_void,
        allocation: *mut c_void,
        protect: u32,
        partition: u16,
        region: usize,
        state: u32,
        protection: u32,
        kind: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenFileMappingW(access: u32, inherit: i32, name: *const u16) -> Handle;
        fn MapViewOfFile(
            mapping: Handle,
            access: u32,
            high: u32,
            low: u32,
            size: usize,
        ) -> *mut c_void;
        fn UnmapViewOfFile(base: *const c_void) -> i32;
        fn CloseHandle(h: Handle) -> i32;
        fn VirtualQuery(address: *const c_void, buffer: *mut MemoryInfo, length: usize) -> usize;
    }
    unsafe {
        let name: Vec<u16> = "MAHMSharedMemory".encode_utf16().chain(Some(0)).collect();
        let handle = OpenFileMappingW(4, 0, name.as_ptr());
        ensure!(!handle.is_null(), "Afterburner 未运行或共享内存无权限");
        let view = MapViewOfFile(handle, 4, 0, 0, 0);
        if view.is_null() {
            CloseHandle(handle);
            anyhow::bail!("共享内存不可读")
        }
        let mut info: MemoryInfo = std::mem::zeroed();
        let result = (|| {
            ensure!(
                VirtualQuery(view, &mut info, size_of::<MemoryInfo>()) > 0
                    && info.region >= 32
                    && info.region <= 300 * 1024 * 1024,
                "共享内存大小无效"
            );
            let bytes = std::slice::from_raw_parts(view as *const u8, info.region);
            let header = bytes[..32].to_vec();
            let copy = bytes.to_vec();
            std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
            ensure!(&bytes[..32] == header.as_slice(), "MAHM 正在更新");
            parse(&copy, crate::now())
        })();
        UnmapViewOfFile(view);
        CloseHandle(handle);
        result
    }
}
#[cfg(not(windows))]
pub fn read() -> Result<Vec<SensorSample>> {
    anyhow::bail!("Afterburner 仅支持 Windows")
}
pub fn display(sample: Option<&SensorSample>, now: i64, integer: bool) -> String {
    match sample {
        Some(s) if s.status == "ok" && now - s.timestamp <= 5000 && s.value.is_some() => {
            let value = s.value.unwrap();
            format!(
                "{:.*}{}{}",
                if integer { 0 } else { 1 },
                value,
                if s.unit.is_empty() { "" } else { " " },
                s.unit
            )
        }
        _ => "--".into(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_truncated_map() {
        let mut b = vec![0u8; 32];
        for (at, n) in [
            (0, 0x4d41484du32),
            (4, 0x20000),
            (8, 32),
            (12, 1),
            (16, 1324),
            (28, 1304),
        ] {
            b[at..at + 4].copy_from_slice(&n.to_le_bytes());
        }
        assert!(parse(&b, 0).is_err());
    }
    #[test]
    fn stale_never_live() {
        let s = SensorSample {
            value: Some(42.),
            status: "ok".into(),
            timestamp: 0,
            ..Default::default()
        };
        assert_eq!(display(Some(&s), 6000, false), "--");
    }
}
