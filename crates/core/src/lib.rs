use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
mod rich_text;
pub use rich_text::*;

pub const IPC_VERSION: u32 = 1;
pub const MAX_SCENES: usize = 32;
pub const MAX_ASSETS: usize = 1000;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FrameFormat {
    Jpeg,
    Rgba8,
    Rgb565Le,
}
pub struct OutputFrame<'a> {
    pub width: u32,
    pub height: u32,
    pub format: FrameFormat,
    pub bytes: &'a [u8],
}
impl OutputFrame<'_> {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.width > 0
                && self.height > 0
                && u64::from(self.width) * u64::from(self.height) <= 16_777_216,
            "帧尺寸无效"
        );
        let pixels = self.width as usize * self.height as usize;
        match self.format {
            FrameFormat::Jpeg => ensure!(
                self.bytes.len() <= 8 * 1024 * 1024
                    && self.bytes.starts_with(&[255, 216])
                    && self.bytes.ends_with(&[255, 217]),
                "JPEG 不完整"
            ),
            FrameFormat::Rgba8 => ensure!(self.bytes.len() == pixels * 4, "RGBA 帧长度不匹配"),
            FrameFormat::Rgb565Le => ensure!(self.bytes.len() == pixels * 2, "RGB565 帧长度不匹配"),
        };
        Ok(())
    }
}
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
macro_rules! identity {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub String);
        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.into())
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}
identity!(DeviceId);
identity!(ScreenId);
identity!(SceneId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    Cover,
    #[default]
    Contain,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LayerKind {
    Image,
    Video,
    Text,
    Clock,
    Sensor,
    Bar,
    Rect,
    Circle,
    Deepseek,
}
impl LayerKind {
    pub const ALL: [Self; 9] = [
        Self::Image,
        Self::Video,
        Self::Text,
        Self::Clock,
        Self::Sensor,
        Self::Bar,
        Self::Rect,
        Self::Circle,
        Self::Deepseek,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Image => "图片",
            Self::Video => "动态媒体",
            Self::Text => "文字",
            Self::Clock => "时钟",
            Self::Sensor => "传感器",
            Self::Bar => "进度条",
            Self::Rect => "矩形",
            Self::Circle => "圆形",
            Self::Deepseek => "DeepSeek",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Layer {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: LayerKind,
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub rotation: f32,
    pub opacity: f32,
    pub visible: bool,
    pub locked: bool,
    pub color: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rich_text: Option<RichText>,
    pub font_size: f32,
    pub font_family: String,
    pub align: String,
    pub vertical_align: String,
    pub bold: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sensor_id: Option<String>,
    pub min: f64,
    pub max: f64,
    pub fit: Fit,
    pub corner_radius: f32,
    pub glass: bool,
    pub glass_blur: f32,
    pub glass_tint_opacity: f32,
    pub deepseek_metric: String,
    pub bar_background: String,
    pub bar_source: String,
    pub bar_direction: String,
}
impl Default for Layer {
    fn default() -> Self {
        Self {
            id: new_id(),
            kind: LayerKind::Text,
            name: "文字".into(),
            x: 40.,
            y: 40.,
            width: 360.,
            height: 64.,
            rotation: 0.,
            opacity: 1.,
            visible: true,
            locked: false,
            color: "#e6efff".into(),
            text: "自定义文字".into(),
            rich_text: None,
            font_size: 32.,
            font_family: "Microsoft YaHei".into(),
            align: "left".into(),
            vertical_align: "middle".into(),
            bold: false,
            asset_id: None,
            sensor_id: None,
            min: 0.,
            max: 100.,
            fit: Fit::Cover,
            corner_radius: 12.,
            glass: false,
            glass_blur: 12.,
            glass_tint_opacity: 0.18,
            deepseek_metric: "summary".into(),
            bar_background: "#29364c".into(),
            bar_source: "sensor".into(),
            bar_direction: "elapsed".into(),
        }
    }
}
impl Layer {
    pub fn new(kind: LayerKind) -> Self {
        let mut l = Self {
            kind,
            name: kind.label().into(),
            ..Self::default()
        };
        if !matches!(kind, LayerKind::Text) {
            l.text.clear();
        }
        match kind {
            LayerKind::Bar => {
                l.width = 200.;
                l.height = 22.;
            }
            LayerKind::Image | LayerKind::Video | LayerKind::Rect | LayerKind::Circle => {
                l.width = 200.;
                l.height = 200.;
            }
            _ => {}
        }
        l
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.id.is_empty() && self.id.len() <= 100, "图层 ID 无效");
        for v in [
            self.x,
            self.y,
            self.width,
            self.height,
            self.rotation,
            self.opacity,
            self.font_size,
            self.corner_radius,
            self.glass_blur,
            self.glass_tint_opacity,
        ] {
            ensure!(v.is_finite(), "图层数值无效");
        }
        ensure!(
            (4. ..=8192.).contains(&self.width) && (4. ..=8192.).contains(&self.height),
            "图层尺寸超限"
        );
        ensure!(
            self.x.abs() <= 16384. && self.y.abs() <= 16384. && self.rotation.abs() <= 360.,
            "图层位置或方向超限"
        );
        ensure!(
            (0. ..=1.).contains(&self.opacity)
                && (0. ..=1.).contains(&self.glass_tint_opacity)
                && (0. ..=40.).contains(&self.glass_blur),
            "透明度或模糊无效"
        );
        ensure!(
            (8. ..=240.).contains(&self.font_size)
                && self.corner_radius >= 0.
                && self.corner_radius <= 1000.,
            "字体或圆角无效"
        );
        ensure!(self.min.is_finite() && self.max.is_finite(), "进度范围无效");
        ensure!(
            self.text.len() <= 16000 && self.name.len() <= 600 && self.font_family.len() <= 400,
            "文字过长"
        );
        if let Some(rich) = &self.rich_text {
            ensure!(self.kind == LayerKind::Text, "只有文字组件支持混排");
            rich.validate()?;
        }
        ensure!(
            valid_color(&self.color) && valid_color(&self.bar_background),
            "颜色无效"
        );
        ensure!(
            ["left", "center", "right"].contains(&self.align.as_str()),
            "对齐无效"
        );
        ensure!(
            ["top", "middle", "bottom"].contains(&self.vertical_align.as_str()),
            "垂直对齐无效"
        );
        ensure!(
            ["sensor", "deepseek-period"].contains(&self.bar_source.as_str())
                && ["elapsed", "remaining"].contains(&self.bar_direction.as_str()),
            "进度数据源无效"
        );
        ensure!(
            ["summary", "balance", "today", "total", "period"]
                .contains(&self.deepseek_metric.as_str()),
            "DeepSeek 指标无效"
        );
        Ok(())
    }
}
pub fn valid_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit())
}
fn canvas_size() -> u32 {
    480
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    pub id: SceneId,
    pub name: String,
    pub background: String,
    #[serde(default = "canvas_size")]
    pub width: u32,
    #[serde(default = "canvas_size")]
    pub height: u32,
    pub layers: Vec<Layer>,
}
impl Scene {
    pub fn blank(name: &str, width: u32, height: u32) -> Self {
        Self {
            id: SceneId(new_id()),
            name: name.into(),
            background: "#101827".into(),
            width,
            height,
            layers: vec![],
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.width > 0
                && self.height > 0
                && self.width <= 8192
                && self.height <= 8192
                && u64::from(self.width) * u64::from(self.height) <= 16_777_216,
            "画布尺寸超限"
        );
        ensure!(
            !self.id.0.is_empty()
                && !self.name.is_empty()
                && self.name.len() <= 400
                && valid_color(&self.background),
            "场景无效"
        );
        ensure!(self.layers.len() <= 128, "图层超过 128");
        let mut ids = BTreeSet::new();
        let mut videos = 0;
        for l in &self.layers {
            l.validate()?;
            ensure!(ids.insert(&l.id), "图层 ID 重复");
            if l.kind == LayerKind::Video && l.visible {
                videos += 1;
            }
        }
        ensure!(videos <= 1, "每个场景最多一个可见动态图层");
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub name: String,
    pub file: String,
    pub kind: String,
    pub width: u32,
    pub height: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
}
pub fn valid_media_file(file: &str) -> bool {
    let Some((id, ext)) = file.rsplit_once('.') else {
        return false;
    };
    !id.is_empty()
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && ["png", "mp4"].contains(&ext)
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub brightness: u8,
    pub direction: u8,
    pub power: bool,
    pub auto_start: bool,
    pub quality: u8,
    pub render_fps: u8,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            brightness: 80,
            direction: 0,
            power: true,
            auto_start: false,
            quality: 70,
            render_fps: 30,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.brightness <= 100
                && self.direction <= 3
                && (40..=90).contains(&self.quality)
                && (1..=30).contains(&self.render_fps),
            "设备设置超限"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputBinding {
    pub device_id: DeviceId,
    pub screen_id: ScreenId,
    pub scene_id: SceneId,
    #[serde(default)]
    pub fit: Fit,
    #[serde(default)]
    pub settings: Settings,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DeepSeekPreferences {
    pub currency: String,
    pub holidays: Vec<String>,
    pub calendar_years: Vec<i32>,
}
impl Default for DeepSeekPreferences {
    fn default() -> Self {
        Self {
            currency: "CNY".into(),
            holidays: [
                (1, 1, 3),
                (2, 15, 23),
                (4, 4, 6),
                (5, 1, 5),
                (6, 19, 21),
                (9, 25, 27),
                (10, 1, 7),
            ]
            .into_iter()
            .flat_map(|(m, a, b)| (a..=b).map(move |d| format!("2026-{m:02}-{d:02}")))
            .collect(),
            calendar_years: vec![2026],
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub version: u32,
    pub scenes: Vec<Scene>,
    pub active_scene_id: SceneId,
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub deepseek: DeepSeekPreferences,
    #[serde(default)]
    pub bindings: Vec<OutputBinding>,
    #[serde(default)]
    pub migrated_from: Option<String>,
}
impl Default for AppState {
    fn default() -> Self {
        let mut scene = Scene::blank("像素启航", 480, 480);
        scene.layers.push(Layer {
            text: "PixelFerry".into(),
            x: 32.,
            y: 40.,
            width: 416.,
            height: 80.,
            font_size: 54.,
            bold: true,
            color: "#89e4cc".into(),
            ..Layer::default()
        });
        scene.layers.push(Layer {
            kind: LayerKind::Clock,
            name: "时钟".into(),
            x: 32.,
            y: 160.,
            width: 416.,
            height: 100.,
            font_size: 70.,
            ..Layer::default()
        });
        scene.layers.push(Layer {
            text: "把画面送往不同 USB 设备".into(),
            x: 32.,
            y: 320.,
            width: 416.,
            height: 60.,
            font_size: 23.,
            ..Layer::default()
        });
        Self {
            version: 2,
            active_scene_id: scene.id.clone(),
            scenes: vec![scene],
            assets: vec![],
            settings: Settings::default(),
            deepseek: DeepSeekPreferences::default(),
            bindings: vec![],
            migrated_from: None,
        }
    }
}
impl AppState {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 2, "不支持的配置版本");
        ensure!(
            !self.scenes.is_empty()
                && self.scenes.len() <= MAX_SCENES
                && self.assets.len() <= MAX_ASSETS,
            "场景或素材数量超限"
        );
        self.settings.validate()?;
        let mut scenes = BTreeSet::new();
        for s in &self.scenes {
            s.validate()?;
            ensure!(scenes.insert(&s.id), "场景 ID 重复");
        }
        ensure!(scenes.contains(&self.active_scene_id), "活动场景不存在");
        let mut assets = BTreeMap::new();
        let mut files = BTreeSet::new();
        for a in &self.assets {
            ensure!(
                !a.id.is_empty()
                    && a.id.len() <= 100
                    && valid_media_file(&a.file)
                    && a.width > 0
                    && a.height > 0
                    && a.width <= 8192
                    && a.height <= 8192,
                "素材无效"
            );
            ensure!(
                (a.kind == "image" && a.file.ends_with(".png"))
                    || (a.kind == "video" && a.file.ends_with(".mp4")),
                "素材格式无效"
            );
            ensure!(
                a.duration
                    .is_none_or(|n| n.is_finite() && (0. ..=600.).contains(&n)),
                "素材时长无效"
            );
            ensure!(
                assets.insert(&a.id, a).is_none() && files.insert(a.file.to_lowercase()),
                "素材重复"
            );
        }
        for s in &self.scenes {
            for l in &s.layers {
                if let Some(id) = &l.asset_id {
                    let Some(a) = assets.get(id) else {
                        bail!("场景引用的素材不存在")
                    };
                    ensure!(
                        !matches!(l.kind, LayerKind::Image | LayerKind::Video)
                            || a.kind
                                == if l.kind == LayerKind::Video {
                                    "video"
                                } else {
                                    "image"
                                },
                        "素材类型不匹配"
                    );
                }
            }
        }
        let mut bindings = BTreeSet::new();
        ensure!(self.bindings.len() <= 8, "首版最多绑定 8 块屏幕");
        ensure!(
            self.bindings
                .iter()
                .filter(|b| b.settings.power
                    && self.scenes.iter().any(|s| s.id == b.scene_id
                        && s.layers
                            .iter()
                            .any(|l| l.visible && l.kind == LayerKind::Video)))
                .count()
                <= 4,
            "首版最多同时播放 4 路动态输出"
        );
        for b in &self.bindings {
            ensure!(scenes.contains(&b.scene_id), "绑定场景不存在");
            ensure!(
                !b.device_id.0.is_empty()
                    && !b.screen_id.0.is_empty()
                    && bindings.insert((&b.device_id, &b.screen_id)),
                "屏幕绑定重复或无效"
            );
            b.settings.validate()?;
        }
        ensure!(
            ["CNY", "USD"].contains(&self.deepseek.currency.as_str())
                && self.deepseek.holidays.len() <= 1500
                && self.deepseek.calendar_years.len() <= 75,
            "DeepSeek 偏好无效"
        );
        ensure!(
            self.deepseek
                .holidays
                .iter()
                .all(|d| d.len() == 10 && chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok())
                && self
                    .deepseek
                    .calendar_years
                    .iter()
                    .all(|y| (2026..=2100).contains(y)),
            "日历日期格式无效"
        );
        Ok(())
    }
    pub fn active_scene(&self) -> &Scene {
        self.scenes
            .iter()
            .find(|s| s.id == self.active_scene_id)
            .unwrap_or(&self.scenes[0])
    }
    pub fn active_scene_mut(&mut self) -> &mut Scene {
        let i = self
            .scenes
            .iter()
            .position(|s| s.id == self.active_scene_id)
            .unwrap_or(0);
        &mut self.scenes[i]
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenDescriptor {
    pub id: ScreenId,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub brightness: bool,
    pub direction: bool,
    pub power: bool,
    pub encoding: FrameFormat,
    pub repeat_fps: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDescriptor {
    pub id: DeviceId,
    pub adapter: String,
    pub name: String,
    pub serial: Option<String>,
    pub container: Option<String>,
    pub screens: Vec<ScreenDescriptor>,
    pub requires_confirmation: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStatus {
    pub device_id: DeviceId,
    pub state: String,
    pub message: String,
    pub fps: f64,
    pub bytes_per_second: f64,
    pub frames: u64,
    pub errors: u64,
    pub screen_frames: BTreeMap<ScreenId, u64>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SensorSample {
    pub id: String,
    pub name: String,
    pub unit: String,
    pub value: Option<f64>,
    pub timestamp: i64,
    pub status: String,
    pub source_id: u32,
    pub gpu_index: u32,
    pub device: String,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
impl Bounds {
    pub fn intersects(self, b: Self) -> bool {
        self.x <= b.x + b.width
            && self.x + self.width >= b.x
            && self.y <= b.y + b.height
            && self.y + self.height >= b.y
    }
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
}
pub fn layer_bounds(l: &Layer) -> Bounds {
    let (sin, cos) = l.rotation.to_radians().sin_cos();
    let pts = [(0., 0.), (l.width, 0.), (0., l.height), (l.width, l.height)]
        .map(|(x, y)| (l.x + x * cos - y * sin, l.y + x * sin + y * cos));
    let x = pts.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
    let y = pts.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
    Bounds {
        x,
        y,
        width: pts.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max) - x,
        height: pts.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max) - y,
    }
}
pub fn union_bounds(boxes: &[Bounds]) -> Bounds {
    if boxes.is_empty() {
        return Bounds::default();
    }
    let x = boxes.iter().map(|b| b.x).fold(f32::INFINITY, f32::min);
    let y = boxes.iter().map(|b| b.y).fold(f32::INFINITY, f32::min);
    Bounds {
        x,
        y,
        width: boxes
            .iter()
            .map(|b| b.x + b.width)
            .fold(f32::NEG_INFINITY, f32::max)
            - x,
        height: boxes
            .iter()
            .map(|b| b.y + b.height)
            .fold(f32::NEG_INFINITY, f32::max)
            - y,
    }
}
pub fn snap(
    moving: Bounds,
    others: &[Bounds],
    width: f32,
    height: f32,
    threshold: f32,
) -> (f32, f32, Option<f32>, Option<f32>) {
    fn axis(
        start: f32,
        size: f32,
        max: f32,
        others: impl Iterator<Item = (f32, f32)>,
        threshold: f32,
    ) -> (f32, Option<f32>) {
        let mut targets = vec![0., max / 2., max];
        for (s, n) in others {
            targets.extend([s, s + n / 2., s + n]);
        }
        let mut best = (threshold + 1., 0., None);
        for a in [start, start + size / 2., start + size] {
            for t in &targets {
                let d = *t - a;
                if d.abs() <= threshold && d.abs() < best.0 {
                    best = (d.abs(), d, Some(*t));
                }
            }
        }
        (best.1, best.2)
    }
    let (x, v) = axis(
        moving.x,
        moving.width,
        width,
        others.iter().map(|b| (b.x, b.width)),
        threshold,
    );
    let (y, h) = axis(
        moving.y,
        moving.height,
        height,
        others.iter().map(|b| (b.y, b.height)),
        threshold,
    );
    (x, y, v, h)
}
#[derive(Default)]
pub struct History {
    undo: Vec<AppState>,
    redo: Vec<AppState>,
}
impl History {
    pub fn record(&mut self, before: &AppState) {
        if self.undo.last() != Some(before) {
            self.undo.push(before.clone());
            if self.undo.len() > 100 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }
    pub fn undo(&mut self, current: &mut AppState) -> bool {
        if let Some(mut s) = self.undo.pop() {
            self.redo.push(current.clone());
            for asset in &current.assets {
                if !s.assets.iter().any(|a| a.id == asset.id) {
                    s.assets.push(asset.clone());
                }
            }
            *current = s;
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self, current: &mut AppState) -> bool {
        if let Some(mut s) = self.redo.pop() {
            self.undo.push(current.clone());
            for asset in &current.assets {
                if !s.assets.iter().any(|a| a.id == asset.id) {
                    s.assets.push(asset.clone());
                }
            }
            *current = s;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_valid() {
        AppState::default().validate().unwrap();
    }
    #[test]
    fn disallow_duplicate_bindings_and_dynamic_layers() {
        let mut s = AppState::default();
        s.scenes[0]
            .layers
            .extend([Layer::new(LayerKind::Video), Layer::new(LayerKind::Video)]);
        assert!(s.validate().is_err());
    }
    #[test]
    fn arbitrary_canvas_snap() {
        let b = Bounds {
            x: 318.,
            y: 90.,
            width: 100.,
            height: 100.,
        };
        let (dx, _, line, _) = snap(b, &[], 640., 480., 5.);
        assert_eq!(dx, 2.);
        assert_eq!(line, Some(320.));
    }
    #[test]
    fn history_restores_transaction() {
        let mut s = AppState::default();
        let old = s.clone();
        let mut h = History::default();
        h.record(&s);
        s.scenes[0].layers[0].x += 5.;
        assert!(h.undo(&mut s));
        assert_eq!(old, s);
        assert!(h.redo(&mut s));
        assert_ne!(old, s);
    }
}
