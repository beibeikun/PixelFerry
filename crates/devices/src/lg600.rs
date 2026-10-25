use crate::{Adapter, Connection};
use anyhow::{ensure, Result};
use pixelferry_core::*;
pub const VID: u16 = 0x1a2c;
pub const PID: u16 = 0x1520;
pub const USAGE_PAGE: u16 = 0xff06;
pub const COMMAND_LENGTH: usize = 441;
pub const IMAGE_LENGTH: usize = 1025;
pub const CHUNK: usize = 1020;
pub fn packetize(jpeg: &[u8]) -> Result<impl Iterator<Item = Vec<u8>> + '_> {
    ensure!(
        jpeg.len() > 100 && jpeg.len() <= 8 * 1024 * 1024,
        "JPEG 长度不合法"
    );
    ensure!(
        jpeg.starts_with(&[255, 216]) && jpeg.ends_with(&[255, 217]),
        "需要完整 JPEG 帧"
    );
    let count = jpeg.len().div_ceil(CHUNK);
    Ok(jpeg.chunks(CHUNK).enumerate().map(move |(i, chunk)| {
        let mut packet = vec![0; IMAGE_LENGTH];
        packet[1] = 8;
        let header = if i == 0 { 0x800000 | count } else { i };
        packet[2..5].copy_from_slice(&(header as u32).to_le_bytes()[..3]);
        packet[5..5 + chunk.len()].copy_from_slice(chunk);
        packet
    }))
}
#[derive(Clone, Copy)]
pub enum Control {
    Brightness,
    Direction,
    Power,
}
pub fn command(command: Control, value: u8) -> Result<Vec<u8>> {
    let (code, max) = match command {
        Control::Brightness => (0x12, 100),
        Control::Direction => (0x11, 3),
        Control::Power => (0x10, 1),
    };
    ensure!(value <= max, "命令参数不合法");
    let mut p = vec![0; COMMAND_LENGTH];
    p[1] = code;
    p[2] = 1;
    p[4] = 128;
    p[5] = value;
    Ok(p)
}
pub struct Lg600;
impl Adapter for Lg600 {
    fn id(&self) -> &'static str {
        "lg600"
    }
    fn discover(&self) -> Result<Vec<DeviceDescriptor>> {
        #[cfg(windows)]
        {
            crate::windows::discover_lg600()
        }
        #[cfg(not(windows))]
        {
            Ok(vec![])
        }
    }
    fn connect(&self, device: &DeviceDescriptor) -> Result<Box<dyn Connection>> {
        #[cfg(windows)]
        {
            Ok(Box::new(crate::windows::LgConnection::open(device)?))
        }
        #[cfg(not(windows))]
        {
            let _ = device;
            anyhow::bail!("LG600 首版仅支持 Windows")
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reports_roundtrip_and_zero_tail() {
        for length in [101, 1020, 1021, 2040, 2041] {
            let mut source = vec![42; length];
            source[..2].copy_from_slice(&[255, 216]);
            source[length - 2..].copy_from_slice(&[255, 217]);
            let packets: Vec<_> = packetize(&source).unwrap().collect();
            assert_eq!(packets.len(), length.div_ceil(CHUNK));
            assert_eq!(
                &packets[0][2..5],
                &(0x800000u32 | packets.len() as u32).to_le_bytes()[..3]
            );
            let mut result = vec![];
            for (i, p) in packets.iter().enumerate() {
                assert_eq!(p.len(), 1025);
                assert_eq!(&p[..2], &[0, 8]);
                if i > 0 {
                    assert_eq!(&p[2..5], &(i as u32).to_le_bytes()[..3]);
                }
                result.extend_from_slice(&p[5..]);
            }
            assert_eq!(&result[..length], &source);
            assert!(result[length..].iter().all(|n| *n == 0));
        }
    }
    #[test]
    fn controls_match_legacy() {
        assert_eq!(
            &command(Control::Brightness, 80).unwrap()[..6],
            &[0, 18, 1, 0, 128, 80]
        );
        assert!(command(Control::Direction, 4).is_err());
        assert!(packetize(&[0; 101]).is_err());
    }
}
