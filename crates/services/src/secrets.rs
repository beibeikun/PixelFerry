use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{bail, ensure, Context, Result};
use base64::Engine;
use std::{fs, path::Path};
#[cfg(windows)]
fn crypt(input: &[u8], encrypt: bool) -> Result<Vec<u8>> {
    use std::{ffi::c_void, ptr};
    #[repr(C)]
    struct Blob {
        size: u32,
        data: *mut u8,
    }
    #[link(name = "crypt32")]
    extern "system" {
        fn CryptProtectData(
            input: *const Blob,
            description: *const u16,
            entropy: *const Blob,
            reserved: *const c_void,
            prompt: *const c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
        fn CryptUnprotectData(
            input: *const Blob,
            description: *mut *mut u16,
            entropy: *const Blob,
            reserved: *const c_void,
            prompt: *const c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(ptr: *mut c_void) -> *mut c_void;
    }
    ensure!(
        !input.is_empty() && input.len() <= 65536,
        "加密数据长度无效"
    );
    let source = Blob {
        size: input.len() as u32,
        data: input.as_ptr() as *mut u8,
    };
    let mut out = Blob {
        size: 0,
        data: ptr::null_mut(),
    };
    unsafe {
        let success = if encrypt {
            CryptProtectData(
                &source,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                1,
                &mut out,
            )
        } else {
            CryptUnprotectData(
                &source,
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                1,
                &mut out,
            )
        };
        ensure!(success != 0, "Windows 凭据解密失败，请重新输入 Key");
        let result = std::slice::from_raw_parts(out.data, out.size as usize).to_vec();
        LocalFree(out.data.cast());
        Ok(result)
    }
}
#[cfg(not(windows))]
fn crypt(_: &[u8], _: bool) -> Result<Vec<u8>> {
    bail!("首版凭据存储仅支持 Windows")
}
pub fn encrypt(key: &str) -> Result<Vec<u8>> {
    validate_key(key)?;
    let mut bytes = b"PFDP1".to_vec();
    bytes.extend(crypt(key.as_bytes(), true)?);
    Ok(bytes)
}
pub fn decrypt(bytes: &[u8]) -> Result<String> {
    ensure!(bytes.starts_with(b"PFDP1"), "凭据格式不支持");
    let key = String::from_utf8(crypt(&bytes[5..], false)?).context("凭据编码无效")?;
    validate_key(&key)?;
    Ok(key)
}
pub fn validate_key(key: &str) -> Result<()> {
    ensure!(
        !key.is_empty() && key.len() <= 512 && key.bytes().all(|b| (0x21..=0x7e).contains(&b)),
        "API Key 格式无效"
    );
    Ok(())
}
/// Supports Chromium AES-GCM v10 and Electron's legacy DPAPI format. Never logs key material.
pub fn decrypt_legacy(root: &Path) -> Result<String> {
    let bytes = fs::read(root.join("deepseek-key.bin"))?;
    ensure!(bytes.len() <= 65536, "旧凭据过长");
    if bytes.starts_with(b"PFDP1") {
        return decrypt(&bytes);
    }
    if let Ok(raw) = crypt(&bytes, false) {
        let key = String::from_utf8(raw)?;
        validate_key(&key)?;
        return Ok(key);
    }
    if bytes.starts_with(b"v10") || bytes.starts_with(b"v11") {
        if let Ok(raw) = crypt(&bytes[3..], false) {
            let key = String::from_utf8(raw)?;
            validate_key(&key)?;
            return Ok(key);
        }
        let state: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("Local State"))?)?;
        let encoded = state["os_crypt"]["encrypted_key"]
            .as_str()
            .context("缺少旧版系统密钥")?;
        let master = base64::engine::general_purpose::STANDARD.decode(encoded)?;
        ensure!(master.starts_with(b"DPAPI"), "旧密钥格式不支持");
        let master = crypt(&master[5..], false)?;
        ensure!(
            master.len() == 32 && bytes.len() >= 3 + 12 + 16,
            "旧凭据格式无效"
        );
        let cipher =
            Aes256Gcm::new_from_slice(&master).map_err(|_| anyhow::anyhow!("系统密钥无效"))?;
        let raw = cipher
            .decrypt(Nonce::from_slice(&bytes[3..15]), &bytes[15..])
            .map_err(|_| anyhow::anyhow!("旧凭据解密失败，请重新输入 Key"))?;
        let key = String::from_utf8(raw)?;
        validate_key(&key)?;
        return Ok(key);
    }
    bail!("无法解密旧凭据，请重新输入 Key")
}
#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn dpapi_roundtrip() {
        let test_key = "pixel-ferry-test-not-a-real-api-key";
        let encrypted = encrypt(test_key).unwrap();
        assert!(!encrypted
            .windows(test_key.len())
            .any(|w| w == test_key.as_bytes()));
        assert_eq!(decrypt(&encrypted).unwrap(), test_key);
    }
}
