pub mod lg600;
#[cfg(windows)]
pub mod windows;
use anyhow::{ensure, Result};
use pixelferry_core::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

pub trait Connection {
    fn send(&mut self, screen: &ScreenId, frame: &OutputFrame<'_>) -> Result<usize>;
    fn controls(&mut self, screen: &ScreenId, settings: &Settings) -> Result<()>;
}
pub trait Adapter {
    fn id(&self) -> &'static str;
    fn discover(&self) -> Result<Vec<DeviceDescriptor>>;
    fn connect(&self, device: &DeviceDescriptor) -> Result<Box<dyn Connection>>;
}
pub struct Latest<T>(Arc<Mutex<Option<Arc<T>>>>);
impl<T> Clone for Latest<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Default for Latest<T> {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }
}
impl<T> Latest<T> {
    pub fn replace(&self, value: T) {
        *self.0.lock().unwrap() = Some(Arc::new(value));
    }
    pub fn snapshot(&self) -> Option<Arc<T>> {
        self.0.lock().unwrap().clone()
    }
}
pub trait PacketOutput {
    fn write(&mut self, packet: &[u8]) -> Result<usize>;
}
#[derive(Default)]
pub struct FramePump {
    pub frames: u64,
    pub bytes: u64,
    pub errors: u64,
}
impl FramePump {
    pub fn send(
        &mut self,
        output: &mut dyn PacketOutput,
        packets: impl Iterator<Item = Vec<u8>>,
    ) -> Result<()> {
        for packet in packets {
            let mut result = Ok(());
            for attempt in 0..2 {
                result = output.write(&packet).and_then(|n| {
                    ensure!(n == packet.len(), "HID 短写");
                    self.bytes += n as u64;
                    Ok(())
                });
                if result.is_ok() {
                    break;
                }
                self.errors += 1;
                if attempt == 0 {
                    std::thread::sleep(Duration::from_millis(3));
                }
            }
            result?;
        }
        self.frames += 1;
        Ok(())
    }
}
pub struct Simulator;
impl Adapter for Simulator {
    fn id(&self) -> &'static str {
        "simulator"
    }
    fn discover(&self) -> Result<Vec<DeviceDescriptor>> {
        Ok([
            ("sim-square", vec![(480, 480)]),
            ("sim-wide", vec![(640, 480)]),
            ("sim-triple", vec![(330, 330), (330, 330), (330, 330)]),
        ]
        .into_iter()
        .map(|(id, sizes)| DeviceDescriptor {
            id: id.into(),
            adapter: self.id().into(),
            name: format!("模拟设备 · {id}"),
            serial: Some(id.into()),
            container: Some(id.into()),
            requires_confirmation: false,
            screens: sizes
                .into_iter()
                .enumerate()
                .map(|(i, (width, height))| ScreenDescriptor {
                    id: ScreenId(format!("screen-{i}")),
                    name: format!("屏幕 {}", i + 1),
                    width,
                    height,
                    brightness: true,
                    direction: true,
                    power: true,
                    encoding: if id == "sim-wide" {
                        FrameFormat::Rgba8
                    } else if id == "sim-triple" && i == 1 {
                        FrameFormat::Rgb565Le
                    } else {
                        FrameFormat::Jpeg
                    },
                    repeat_fps: 30,
                })
                .collect(),
        })
        .collect())
    }
    fn connect(&self, device: &DeviceDescriptor) -> Result<Box<dyn Connection>> {
        Ok(Box::new(SimConnection {
            screens: device
                .screens
                .iter()
                .map(|s| (s.id.clone(), (s.clone(), Settings::default())))
                .collect(),
            writes: 0,
        }))
    }
}
pub struct SimConnection {
    screens: BTreeMap<ScreenId, (ScreenDescriptor, Settings)>,
    pub writes: u64,
}
impl Connection for SimConnection {
    fn send(&mut self, screen: &ScreenId, frame: &OutputFrame<'_>) -> Result<usize> {
        let (descriptor, _) = self
            .screens
            .get(screen)
            .ok_or_else(|| anyhow::anyhow!("未知模拟屏幕"))?;
        frame.validate()?;
        ensure!(
            frame.width == descriptor.width
                && frame.height == descriptor.height
                && frame.format == descriptor.encoding,
            "帧与屏幕能力不匹配"
        );
        self.writes += 1;
        Ok(frame.bytes.len())
    }
    fn controls(&mut self, screen: &ScreenId, settings: &Settings) -> Result<()> {
        settings.validate()?;
        let s = self
            .screens
            .get_mut(screen)
            .ok_or_else(|| anyhow::anyhow!("未知屏幕"))?;
        s.1 = settings.clone();
        Ok(())
    }
}
pub fn adapter(id: &str) -> Result<Box<dyn Adapter>> {
    match id {
        "simulator" => Ok(Box::new(Simulator)),
        "lg600" => Ok(Box::new(lg600::Lg600)),
        _ => anyhow::bail!("未安装设备适配器: {id}"),
    }
}
pub fn discover(simulate: bool) -> Result<Vec<DeviceDescriptor>> {
    let mut devices = vec![];
    if simulate {
        devices.extend(Simulator.discover()?);
    } else {
        devices.extend(lg600::Lg600.discover()?);
    }
    Ok(devices)
}
pub fn conflicts() -> Vec<String> {
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("tasklist.exe");
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
        if let Ok(out) = cmd.args(["/FO", "CSV", "/NH"]).output() {
            let s = String::from_utf8_lossy(&out.stdout).to_lowercase();
            return ["t'owei.exe", "lg600 studio.exe"]
                .into_iter()
                .filter(|n| s.contains(n))
                .map(str::to_owned)
                .collect();
        }
    }
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_keeps_current_frame_alive() {
        let l = Latest::default();
        l.replace(vec![1; 2041]);
        let first = l.snapshot().unwrap();
        l.replace(vec![2; 1021]);
        assert_eq!(first.len(), 2041);
        assert_eq!(l.snapshot().unwrap().len(), 1021);
    }
    #[test]
    fn all_simulated_screens_independent() {
        let a = Simulator;
        let ds = a.discover().unwrap();
        assert_eq!(ds.iter().map(|d| d.screens.len()).sum::<usize>(), 5);
        for d in ds {
            let mut c = a.connect(&d).unwrap();
            for s in d.screens {
                c.controls(&s.id, &Settings::default()).unwrap();
                let bytes = match s.encoding {
                    FrameFormat::Jpeg => vec![255, 216, 0, 255, 217],
                    FrameFormat::Rgba8 => vec![0; s.width as usize * s.height as usize * 4],
                    FrameFormat::Rgb565Le => vec![0; s.width as usize * s.height as usize * 2],
                };
                assert_eq!(
                    c.send(
                        &s.id,
                        &OutputFrame {
                            width: s.width,
                            height: s.height,
                            format: s.encoding,
                            bytes: &bytes
                        }
                    )
                    .unwrap(),
                    bytes.len()
                );
            }
        }
    }
    #[test]
    fn short_write_retries_once() {
        struct Short(usize);
        impl PacketOutput for Short {
            fn write(&mut self, _: &[u8]) -> Result<usize> {
                self.0 += 1;
                Ok(1)
            }
        }
        let mut o = Short(0);
        let mut p = FramePump::default();
        assert!(p.send(&mut o, vec![vec![0; 1025]].into_iter()).is_err());
        assert_eq!(o.0, 2);
        assert_eq!(p.errors, 2);
        assert_eq!(p.frames, 0);
    }
}
