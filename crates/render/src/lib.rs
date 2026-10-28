use anyhow::{ensure, Context, Result};
use chrono::{Local, TimeZone};
use pixelferry_core::*;
use pixelferry_services::{deepseek, sensors};
use skia_safe::{
    self as sk, AlphaType, Color, ColorType, Data, EncodedImageFormat, Image, ImageInfo, Paint,
    Rect,
};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
};

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub format: FrameFormat,
    pub payload: Vec<u8>,
}
impl Frame {
    pub fn output(&self) -> OutputFrame<'_> {
        OutputFrame {
            width: self.width,
            height: self.height,
            format: self.format,
            bytes: &self.payload,
        }
    }
}
pub struct RenderRequest<'a> {
    pub scene: &'a Scene,
    pub assets: &'a [Asset],
    pub data: &'a DataSnapshot,
    pub prefs: &'a DeepSeekPreferences,
    pub size: (u32, u32),
    pub fit: Fit,
    pub direction: u8,
    pub quality: u8,
    pub format: FrameFormat,
    pub now: i64,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct DataSnapshot {
    pub sensors: Vec<SensorSample>,
    #[serde(default)]
    pub sensor_error: Option<String>,
    pub deepseek: deepseek::Snapshot,
}
pub fn color(value: &str) -> Color {
    let n = u32::from_str_radix(value.trim_start_matches('#'), 16).unwrap_or(0);
    Color::from_argb(255, (n >> 16) as u8, (n >> 8) as u8, n as u8)
}
pub fn fit_transform(sw: f32, sh: f32, tw: f32, th: f32, fit: Fit) -> (f32, f32, f32) {
    let scale = match fit {
        Fit::Contain => (tw / sw).min(th / sh),
        Fit::Cover => (tw / sw).max(th / sh),
    };
    (scale, (tw - sw * scale) / 2., (th - sh * scale) / 2.)
}
struct Video {
    _job: pixelferry_services::process::Job,
    child: Arc<Mutex<Child>>,
    latest: Arc<Mutex<Option<Vec<u8>>>>,
    width: u32,
    height: u32,
}
impl Drop for Video {
    fn drop(&mut self) {
        if let Ok(mut c) = self.child.lock() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}
fn quiet(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
}
impl Video {
    fn start(path: &Path, width: u32, height: u32, ffmpeg: &Path) -> Result<Self> {
        ensure!(
            width > 0 && height > 0 && u64::from(width) * u64::from(height) <= 4_000_000,
            "视频尺寸超限"
        );
        let mut cmd = Command::new(ffmpeg);
        quiet(&mut cmd);
        let mut child = cmd
            .args([
                "-nostdin",
                "-v",
                "error",
                "-re",
                "-stream_loop",
                "-1",
                "-threads",
                "1",
                "-i",
            ])
            .arg(path)
            .args([
                "-map",
                "0:v:0",
                "-vf",
                &format!("scale={width}:{height},fps=30"),
                "-threads",
                "1",
                "-an",
                "-sn",
                "-dn",
                "-pix_fmt",
                "rgba",
                "-f",
                "rawvideo",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("无法启动 FFmpeg 解码器")?;
        let mut stdout = child.stdout.take().unwrap();
        let job = match pixelferry_services::process::Job::attach(&child) {
            Ok(job) => job,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        let latest = Arc::new(Mutex::new(None));
        let output = latest.clone();
        thread::spawn(move || {
            let size = width as usize * height as usize * 4;
            loop {
                let mut frame = vec![0; size];
                if stdout.read_exact(&mut frame).is_err() {
                    break;
                }
                *output.lock().unwrap() = Some(frame);
            }
        });
        Ok(Self {
            _job: job,
            child: Arc::new(Mutex::new(child)),
            latest,
            width,
            height,
        })
    }
    fn image(&self) -> Option<Image> {
        let frame = self.latest.lock().unwrap();
        let pixels = frame.as_ref()?;
        let info = ImageInfo::new(
            (self.width as i32, self.height as i32),
            ColorType::RGBA8888,
            AlphaType::Unpremul,
            None,
        );
        sk::images::raster_from_data(&info, Data::new_copy(pixels), self.width as usize * 4)
    }
}
pub struct Renderer {
    media: PathBuf,
    ffmpeg: PathBuf,
    images: BTreeMap<String, Image>,
    videos: BTreeMap<String, Video>,
    pub notices: Vec<String>,
}
impl Renderer {
    pub fn new(media: &Path, tools: &Path) -> Self {
        Self {
            media: media.into(),
            ffmpeg: tools.join("ffmpeg.exe"),
            images: BTreeMap::new(),
            videos: BTreeMap::new(),
            notices: vec![],
        }
    }
    fn image(&mut self, asset: &Asset) -> Option<Image> {
        if asset.kind == "video" {
            if !self.videos.contains_key(&asset.id) {
                if self.videos.len() >= 4 {
                    self.notice("同时解码的视频已达到上限");
                    return None;
                }
                match Video::start(
                    &self.media.join(&asset.file),
                    asset.width,
                    asset.height,
                    &self.ffmpeg,
                ) {
                    Ok(v) => {
                        self.videos.insert(asset.id.clone(), v);
                    }
                    Err(e) => {
                        self.notice(&e.to_string());
                        return None;
                    }
                }
            }
            return self.videos.get(&asset.id).and_then(Video::image);
        }
        if let Some(i) = self.images.get(&asset.id) {
            return Some(i.clone());
        }
        let image = fs::read(self.media.join(&asset.file))
            .ok()
            .and_then(|b| Image::from_encoded(Data::new_copy(&b)));
        if let Some(i) = &image {
            if self.images.len() >= 32 {
                self.images.clear();
            }
            self.images.insert(asset.id.clone(), i.clone());
        }
        image
    }
    fn notice(&mut self, message: &str) {
        if !self.notices.iter().any(|s| s == message) {
            if self.notices.len() >= 32 {
                self.notices.remove(0);
            }
            self.notices.push(message.into());
        }
    }
    pub fn render(&mut self, request: RenderRequest<'_>) -> Result<Frame> {
        let RenderRequest {
            scene,
            assets,
            data,
            prefs,
            size: (width, height),
            fit,
            direction,
            quality,
            format,
            now,
        } = request;
        ensure!(
            width > 0 && height > 0 && u64::from(width) * u64::from(height) <= 16_777_216,
            "输出尺寸超限"
        );
        ensure!(direction <= 3, "安装方向无效");
        // Fit in the screen's logical orientation before rotating into its native pixels.
        let (logical_width, logical_height) = if direction % 2 == 1 {
            (height, width)
        } else {
            (width, height)
        };
        scene.validate()?;
        let visible: std::collections::BTreeSet<_> = scene
            .layers
            .iter()
            .filter(|l| l.visible && l.kind == LayerKind::Video)
            .filter_map(|l| l.asset_id.clone())
            .collect();
        self.videos.retain(|id, _| visible.contains(id));
        let mut surface =
            sk::surfaces::raster_n32_premul((logical_width as i32, logical_height as i32))
                .context("无法分配画布")?;
        surface.canvas().clear(Color::BLACK);
        let (scale, dx, dy) = fit_transform(
            scene.width as f32,
            scene.height as f32,
            logical_width as f32,
            logical_height as f32,
            fit,
        );
        {
            let canvas = surface.canvas();
            canvas.save();
            canvas.translate((dx, dy));
            canvas.scale((scale, scale));
            canvas.clip_rect(
                Rect::from_wh(scene.width as f32, scene.height as f32),
                None,
                true,
            );
            let mut p = Paint::default();
            p.set_color(color(&scene.background));
            canvas.draw_rect(Rect::from_wh(scene.width as f32, scene.height as f32), &p);
        }
        for layer in scene.layers.iter().filter(|l| l.visible) {
            let backdrop = if layer.glass {
                Some(surface.image_snapshot())
            } else {
                None
            };
            let media = if matches!(layer.kind, LayerKind::Image | LayerKind::Video) {
                layer
                    .asset_id
                    .as_ref()
                    .and_then(|id| assets.iter().find(|a| &a.id == id))
                    .and_then(|a| self.image(a))
            } else {
                None
            };
            let canvas = surface.canvas();
            canvas.save();
            canvas.translate((layer.x, layer.y));
            canvas.rotate(layer.rotation, None);
            canvas.save_layer_alpha(None, (layer.opacity * 255.) as u32);
            let rect = Rect::from_wh(layer.width, layer.height);
            let mut paint = Paint::default();
            paint.set_anti_alias(true);
            paint.set_color(color(&layer.color));
            match layer.kind {
                LayerKind::Rect | LayerKind::Circle => {
                    let radius = layer
                        .corner_radius
                        .min(layer.width / 2.)
                        .min(layer.height / 2.);
                    let circle = layer.kind == LayerKind::Circle;
                    let path = if circle {
                        let mut p = sk::Path::new();
                        p.add_circle(
                            (layer.width / 2., layer.height / 2.),
                            layer.width.min(layer.height) / 2.,
                            None,
                        );
                        p
                    } else {
                        let mut p = sk::Path::new();
                        p.add_round_rect(rect, (radius, radius), None);
                        p
                    };
                    if let Some(backdrop) = backdrop {
                        canvas.save();
                        canvas.clip_path(&path, None, true);
                        canvas.save();
                        canvas.rotate(-layer.rotation, None);
                        canvas.translate((-layer.x, -layer.y));
                        canvas.scale((1. / scale, 1. / scale));
                        canvas.translate((-dx, -dy));
                        let mut blur = Paint::default();
                        blur.set_image_filter(sk::image_filters::blur(
                            (layer.glass_blur * scale, layer.glass_blur * scale),
                            None,
                            None,
                            None,
                        ));
                        canvas.draw_image(backdrop, (0, 0), Some(&blur));
                        canvas.restore();
                        canvas.restore();
                        paint.set_alpha_f(layer.glass_tint_opacity);
                    }
                    canvas.draw_path(&path, &paint);
                }
                LayerKind::Image | LayerKind::Video => {
                    if let Some(image) = media {
                        let (s, x, y) = fit_transform(
                            image.width() as f32,
                            image.height() as f32,
                            layer.width,
                            layer.height,
                            layer.fit,
                        );
                        canvas.save();
                        canvas.clip_rect(rect, None, true);
                        canvas.draw_image_rect(
                            &image,
                            None,
                            Rect::from_xywh(
                                x,
                                y,
                                image.width() as f32 * s,
                                image.height() as f32 * s,
                            ),
                            &Paint::default(),
                        );
                        canvas.restore();
                    } else {
                        paint.set_color(color("#1e293b"));
                        canvas.draw_rect(rect, &paint);
                        draw_text(canvas, "素材载入中", layer);
                    }
                }
                LayerKind::Bar => {
                    let mut ratio = if layer.bar_source == "deepseek-period" {
                        deepseek::progress(now, prefs)
                    } else {
                        let sensor = data
                            .sensors
                            .iter()
                            .find(|s| Some(&s.id) == layer.sensor_id.as_ref());
                        sensor
                            .filter(|s| s.status == "ok" && now - s.timestamp <= 5000)
                            .and_then(|s| s.value)
                            .filter(|_| layer.max > layer.min)
                            .map(|v| ((v - layer.min) / (layer.max - layer.min)).clamp(0., 1.))
                            .unwrap_or(0.)
                    };
                    if layer.bar_source == "deepseek-period" && layer.bar_direction == "remaining" {
                        ratio = 1. - ratio;
                    }
                    paint.set_color(color(&layer.bar_background));
                    canvas.draw_round_rect(rect, layer.height / 2., layer.height / 2., &paint);
                    paint.set_color(color(&layer.color));
                    canvas.save();
                    canvas.clip_rect(
                        Rect::from_wh(layer.width * ratio as f32, layer.height),
                        None,
                        true,
                    );
                    canvas.draw_round_rect(rect, layer.height / 2., layer.height / 2., &paint);
                    canvas.restore();
                }
                _ => {
                    if layer.kind == LayerKind::Text {
                        if let Some(rich) = &layer.rich_text {
                            let runs = pixelferry_services::rich_text::resolve(
                                rich,
                                &data.sensors,
                                &data.deepseek,
                                prefs,
                                now,
                            );
                            draw_runs(canvas, &runs, layer);
                            canvas.restore();
                            canvas.restore();
                            continue;
                        }
                    }
                    let text = match layer.kind {
                        LayerKind::Clock => Local
                            .timestamp_millis_opt(now)
                            .unwrap()
                            .format("%H:%M:%S")
                            .to_string(),
                        LayerKind::Sensor => sensors::display(
                            data.sensors
                                .iter()
                                .find(|s| Some(&s.id) == layer.sensor_id.as_ref()),
                            now,
                            layer.text == "integer",
                        ),
                        LayerKind::Deepseek => {
                            deepseek::text(&layer.deepseek_metric, &data.deepseek, prefs, now)
                        }
                        _ => layer.text.clone(),
                    };
                    draw_text(canvas, &text, layer);
                }
            }
            canvas.restore();
            canvas.restore();
        }
        surface.canvas().restore();
        if direction != 0 {
            // Rotate the completed scene so glass/backdrop sampling keeps its scene coordinates.
            let mut output = sk::surfaces::raster_n32_premul((width as i32, height as i32))
                .context("无法分配旋转画布")?;
            let canvas = output.canvas();
            let offset = match direction {
                1 => (width as f32, 0.),
                2 => (width as f32, height as f32),
                _ => (0., height as f32),
            };
            canvas.translate(offset);
            canvas.rotate(direction as f32 * 90., None);
            canvas.draw_image(surface.image_snapshot(), (0, 0), None);
            surface = output;
        }
        let info = ImageInfo::new(
            (width as i32, height as i32),
            ColorType::RGBA8888,
            AlphaType::Unpremul,
            None,
        );
        let mut rgba = vec![0; width as usize * height as usize * 4];
        ensure!(
            surface.read_pixels(&info, &mut rgba, width as usize * 4, (0, 0)),
            "无法读取画面"
        );
        let payload = match format {
            FrameFormat::Jpeg => surface
                .image_snapshot()
                .encode(None, EncodedImageFormat::JPEG, quality as u32)
                .context("JPEG 编码失败")?
                .as_bytes()
                .to_vec(),
            FrameFormat::Rgba8 => rgba.clone(),
            FrameFormat::Rgb565Le => rgb565(&rgba),
        };
        Ok(Frame {
            width,
            height,
            rgba,
            format,
            payload,
        })
    }
}
fn rgb565(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len() / 2);
    for p in rgba.as_chunks::<4>().0 {
        let n = ((p[0] as u16 >> 3) << 11) | ((p[1] as u16 >> 2) << 5) | (p[2] as u16 >> 3);
        out.extend(n.to_le_bytes());
    }
    out
}
fn draw_text(canvas: &sk::Canvas, text: &str, layer: &Layer) {
    draw_runs(
        canvas,
        &[pixelferry_services::rich_text::ResolvedRun {
            text: text.into(),
            style: RichStyle::default(),
            font_scale: 1.,
        }],
        layer,
    );
}
fn draw_runs(
    canvas: &sk::Canvas,
    runs: &[pixelferry_services::rich_text::ResolvedRun],
    layer: &Layer,
) {
    use sk::textlayout::{FontCollection, ParagraphBuilder, ParagraphStyle, TextAlign, TextStyle};
    let mut fonts = FontCollection::new();
    fonts.set_default_font_manager(sk::FontMgr::default(), None);
    let mut style = TextStyle::new();
    style.set_color(color(&layer.color));
    style.set_font_size(layer.font_size);
    style.set_font_families(&[layer.font_family.as_str(), "Microsoft YaHei", "Segoe UI"]);
    style.set_font_style(if layer.bold {
        sk::FontStyle::bold()
    } else {
        sk::FontStyle::normal()
    });
    let mut paragraph_style = ParagraphStyle::new();
    paragraph_style.set_text_style(&style);
    paragraph_style.set_text_align(match layer.align.as_str() {
        "center" => TextAlign::Center,
        "right" => TextAlign::Right,
        _ => TextAlign::Left,
    });
    let mut builder = ParagraphBuilder::new(&paragraph_style, fonts);
    for run in runs {
        let mut run_style = style.clone();
        if let Some(color_value) = &run.style.color {
            run_style.set_color(color(color_value));
        }
        run_style.set_font_size(run.style.font_size.unwrap_or(layer.font_size) * run.font_scale);
        if let Some(family) = &run.style.font_family {
            run_style.set_font_families(&[family.as_str(), "Microsoft YaHei", "Segoe UI"]);
        }
        if let Some(bold) = run.style.bold {
            run_style.set_font_style(if bold {
                sk::FontStyle::bold()
            } else {
                sk::FontStyle::normal()
            });
        }
        builder.push_style(&run_style);
        builder.add_text(&run.text);
        builder.pop();
    }
    let mut paragraph = builder.build();
    paragraph.layout(layer.width);
    canvas.save();
    canvas.clip_rect(Rect::from_wh(layer.width, layer.height), None, true);
    paragraph.paint(
        canvas,
        (
            0.,
            vertical_offset(&layer.vertical_align, layer.height, paragraph.height()),
        ),
    );
    canvas.restore();
}
fn vertical_offset(align: &str, height: f32, text_height: f32) -> f32 {
    let space = (height - text_height).max(0.);
    match align {
        "top" => 0.,
        "bottom" => space,
        _ => space / 2.,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn horizontal_and_vertical_text_alignment_are_independent() {
        let mut scene = Scene::blank("alignment", 320, 160);
        scene.background = "#000000".into();
        scene.layers.push(Layer {
            x: 10.,
            y: 10.,
            width: 300.,
            height: 140.,
            color: "#ffffff".into(),
            text: "A".into(),
            ..Layer::new(LayerKind::Text)
        });
        let mut rows = vec![];
        for vertical in ["top", "middle", "bottom"] {
            let mut columns = vec![];
            for horizontal in ["left", "center", "right"] {
                scene.layers[0].vertical_align = vertical.into();
                scene.layers[0].align = horizontal.into();
                let frame =
                    render_orientation(&scene, (320, 160), Fit::Contain, 0, FrameFormat::Rgba8);
                let pixels: Vec<_> = frame
                    .rgba
                    .chunks_exact(4)
                    .enumerate()
                    .filter(|(_, p)| p[0] > 100)
                    .map(|(i, _)| ((i % 320) as u32, (i / 320) as u32))
                    .collect();
                columns.push((
                    pixels.iter().map(|p| p.0).min().unwrap(),
                    pixels.iter().map(|p| p.1).min().unwrap(),
                ));
            }
            assert!(columns[0].0 < columns[1].0 && columns[1].0 < columns[2].0);
            assert_eq!(columns[0].1, columns[1].1);
            assert_eq!(columns[1].1, columns[2].1);
            rows.push(columns[0].1);
        }
        assert!(rows[0] < rows[1] && rows[1] < rows[2]);
        assert_eq!(vertical_offset("bottom", 20., 40.), 0.);
    }
    #[test]
    fn fit_non_square() {
        assert_eq!(
            fit_transform(480., 480., 640., 480., Fit::Contain),
            (1., 80., 0.)
        );
        let (s, x, y) = fit_transform(480., 480., 640., 480., Fit::Cover);
        assert!((s - 4. / 3.).abs() < 0.001 && x == 0. && y < 0.);
    }
    #[test]
    fn render_pixels_and_complete_jpeg() {
        let mut s = Scene::blank("test", 100, 100);
        s.background = "#ff0000".into();
        let mut r = Renderer::new(Path::new("."), Path::new("."));
        let f = r
            .render(RenderRequest {
                scene: &s,
                assets: &[],
                data: &DataSnapshot::default(),
                prefs: &DeepSeekPreferences::default(),
                size: (200, 100),
                fit: Fit::Contain,
                direction: 0,
                quality: 70,
                format: FrameFormat::Jpeg,
                now: 0,
            })
            .unwrap();
        assert_eq!(&f.rgba[..4], &[0, 0, 0, 255]);
        assert_eq!(&f.rgba[50 * 4..50 * 4 + 4], &[255, 0, 0, 255]);
        assert!(f.payload.starts_with(&[255, 216]) && f.payload.ends_with(&[255, 217]));
    }
    #[test]
    fn rgb565_wire_order() {
        assert_eq!(
            rgb565(&[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255]),
            vec![0, 248, 224, 7, 31, 0]
        );
    }
    #[test]
    fn rich_text_inherits_styles_and_mixed_runs_render_in_all_formats_with_clipping() {
        let mut scene = Scene::blank("rich", 320, 160);
        scene.background = "#000000".into();
        let mut layer = Layer {
            x: 0.,
            y: 0.,
            width: 180.,
            height: 140.,
            font_size: 24.,
            color: "#ffffff".into(),
            text: "Hello 世界".into(),
            ..Layer::new(LayerKind::Text)
        };
        scene.layers.push(layer.clone());
        let plain = render_orientation(&scene, (320, 160), Fit::Contain, 0, FrameFormat::Rgba8);
        layer.rich_text = Some(RichText {
            version: 1,
            runs: vec![RichRun::Text {
                text: layer.text.clone(),
                style: RichStyle::default(),
            }],
        });
        scene.layers[0] = layer.clone();
        assert_eq!(
            plain.rgba,
            render_orientation(&scene, (320, 160), Fit::Contain, 0, FrameFormat::Rgba8).rgba
        );
        layer.rich_text = Some(RichText {
            version: 1,
            runs: vec![
                RichRun::Text {
                    text: "AAA ".into(),
                    style: RichStyle {
                        color: Some("#ff0000".into()),
                        font_size: Some(40.),
                        ..Default::default()
                    },
                },
                RichRun::Text {
                    text: "BBB\n".into(),
                    style: RichStyle {
                        color: Some("#00ff00".into()),
                        bold: Some(true),
                        ..Default::default()
                    },
                },
                RichRun::Text {
                    text: "CCC CCC CCC CCC CCC".into(),
                    style: RichStyle {
                        color: Some("#0000ff".into()),
                        font_size: Some(48.),
                        ..Default::default()
                    },
                },
            ],
        });
        scene.layers[0] = layer;
        let rgba = render_orientation(&scene, (320, 160), Fit::Contain, 0, FrameFormat::Rgba8);
        for color in [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]] {
            assert!(rgba.rgba.chunks_exact(4).any(|p| p == color));
        }
        for y in 0..160 {
            for x in 0..320 {
                if x >= 180 || y >= 140 {
                    let i = (y * 320 + x) * 4;
                    assert_eq!(&rgba.rgba[i..i + 4], &[0, 0, 0, 255]);
                }
            }
        }
        let rgb = render_orientation(&scene, (320, 160), Fit::Contain, 0, FrameFormat::Rgb565Le);
        let jpeg = render_orientation(&scene, (320, 160), Fit::Contain, 0, FrameFormat::Jpeg);
        assert_eq!(rgb.rgba, rgba.rgba);
        assert_eq!(rgb.payload, rgb565(&rgba.rgba));
        assert_eq!(jpeg.rgba, rgba.rgba);
        assert!(Image::from_encoded(Data::new_copy(&jpeg.payload)).is_some());
    }
    #[test]
    fn dynamic_fields_render_the_same_pixels_as_their_resolved_text() {
        let mut scene = Scene::blank("dynamic", 480, 160);
        let mut layer = Layer {
            x: 0.,
            y: 0.,
            width: 480.,
            height: 120.,
            text: "余额：¥1.01 / 65.25 °C".into(),
            ..Layer::new(LayerKind::Text)
        };
        scene.layers.push(layer.clone());
        let expected = render_orientation(&scene, (480, 160), Fit::Contain, 0, FrameFormat::Rgba8);
        layer.rich_text = Some(RichText {
            version: 1,
            runs: vec![
                RichRun::Text {
                    text: "余额：".into(),
                    style: RichStyle::default(),
                },
                RichRun::Deepseek {
                    metric: "balance".into(),
                    decimals: 2,
                    show_currency: true,
                    affix_scale: None,
                    style: RichStyle::default(),
                },
                RichRun::Text {
                    text: " / ".into(),
                    style: RichStyle::default(),
                },
                RichRun::Sensor {
                    sensor_id: "gpu".into(),
                    label: "GPU".into(),
                    decimals: 2,
                    show_unit: true,
                    affix_scale: None,
                    style: RichStyle::default(),
                },
            ],
        });
        scene.layers[0] = layer;
        let data = DataSnapshot {
            sensors: vec![SensorSample {
                id: "gpu".into(),
                name: "GPU".into(),
                unit: "°C".into(),
                value: Some(65.25),
                timestamp: 1000,
                status: "ok".into(),
                source_id: 0,
                gpu_index: 0,
                device: String::new(),
            }],
            deepseek: deepseek::Snapshot {
                configured: true,
                status: "ok".into(),
                timestamp: 1000,
                balances: vec![deepseek::Balance {
                    currency: "CNY".into(),
                    total: "1.005".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let actual = Renderer::new(Path::new("."), Path::new("."))
            .render(RenderRequest {
                scene: &scene,
                assets: &[],
                data: &data,
                prefs: &DeepSeekPreferences::default(),
                size: (480, 160),
                fit: Fit::Contain,
                direction: 0,
                quality: 90,
                format: FrameFormat::Rgba8,
                now: 1000,
            })
            .unwrap();
        assert_eq!(actual.rgba, expected.rgba);
    }
    fn quadrant_scene() -> Scene {
        let mut scene = Scene::blank("orientation", 64, 48);
        for (x, y, color) in [
            (0., 0., "#ff0000"),
            (32., 0., "#00ff00"),
            (0., 24., "#0000ff"),
            (32., 24., "#ffffff"),
        ] {
            scene.layers.push(Layer {
                x,
                y,
                width: 32.,
                height: 24.,
                color: color.into(),
                corner_radius: 0.,
                ..Layer::new(LayerKind::Rect)
            });
        }
        scene
    }
    #[test]
    fn smaller_affixes_match_separate_font_sizes_on_a_shared_baseline() {
        let mut scene = Scene::blank("affixes", 480, 160);
        let numeric = RichStyle {
            font_size: Some(40.),
            ..Default::default()
        };
        let small = RichStyle {
            font_size: Some(20.),
            ..Default::default()
        };
        let rich = RichText {
            version: 1,
            runs: vec![
                RichRun::Deepseek {
                    metric: "balance".into(),
                    decimals: 2,
                    show_currency: true,
                    affix_scale: Some(0.5),
                    style: numeric.clone(),
                },
                RichRun::Text {
                    text: " / ".into(),
                    style: RichStyle::default(),
                },
                RichRun::Sensor {
                    sensor_id: "gpu".into(),
                    label: "GPU".into(),
                    decimals: 1,
                    show_unit: true,
                    affix_scale: Some(0.5),
                    style: numeric.clone(),
                },
            ],
        };
        scene.layers.push(Layer {
            x: 0.,
            y: 0.,
            width: 480.,
            height: 140.,
            vertical_align: "bottom".into(),
            rich_text: Some(rich),
            ..Layer::new(LayerKind::Text)
        });
        let data = DataSnapshot {
            sensors: vec![SensorSample {
                id: "gpu".into(),
                name: "GPU".into(),
                unit: "°C".into(),
                value: Some(65.),
                timestamp: 1000,
                status: "ok".into(),
                source_id: 0,
                gpu_index: 0,
                device: String::new(),
            }],
            deepseek: deepseek::Snapshot {
                configured: true,
                status: "ok".into(),
                timestamp: 1000,
                balances: vec![deepseek::Balance {
                    currency: "CNY".into(),
                    total: "12.5".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let render = |scene: &Scene, format| {
            Renderer::new(Path::new("."), Path::new("."))
                .render(RenderRequest {
                    scene,
                    assets: &[],
                    data: &data,
                    prefs: &DeepSeekPreferences::default(),
                    size: (480, 160),
                    fit: Fit::Contain,
                    direction: 0,
                    quality: 90,
                    format,
                    now: 1000,
                })
                .unwrap()
        };
        let actual = render(&scene, FrameFormat::Rgba8);
        let mut expected = scene.clone();
        expected.layers[0].rich_text = Some(RichText {
            version: 1,
            runs: vec![
                RichRun::Text {
                    text: "¥".into(),
                    style: small.clone(),
                },
                RichRun::Text {
                    text: "12.50".into(),
                    style: numeric.clone(),
                },
                RichRun::Text {
                    text: " / ".into(),
                    style: RichStyle::default(),
                },
                RichRun::Text {
                    text: "65.0".into(),
                    style: numeric,
                },
                RichRun::Text {
                    text: " °C".into(),
                    style: small,
                },
            ],
        });
        assert_eq!(actual.rgba, render(&expected, FrameFormat::Rgba8).rgba);
        for format in [FrameFormat::Jpeg, FrameFormat::Rgb565Le] {
            assert_eq!(render(&scene, format).rgba, actual.rgba);
        }
    }
    fn render_orientation(
        scene: &Scene,
        size: (u32, u32),
        fit: Fit,
        direction: u8,
        format: FrameFormat,
    ) -> Frame {
        Renderer::new(Path::new("."), Path::new("."))
            .render(RenderRequest {
                scene,
                assets: &[],
                data: &DataSnapshot::default(),
                prefs: &DeepSeekPreferences::default(),
                size,
                fit,
                direction,
                quality: 90,
                format,
                now: 0,
            })
            .unwrap()
    }
    #[test]
    fn all_installation_directions_fit_then_rotate_without_changing_screen_dimensions() {
        let scene = quadrant_scene();
        for (width, height) in [(64, 64), (96, 64)] {
            for fit in [Fit::Contain, Fit::Cover] {
                for direction in 0..=3 {
                    let logical = if direction % 2 == 1 {
                        (height, width)
                    } else {
                        (width, height)
                    };
                    let base = render_orientation(&scene, logical, fit, 0, FrameFormat::Rgba8);
                    let frame = render_orientation(
                        &scene,
                        (width, height),
                        fit,
                        direction,
                        FrameFormat::Rgba8,
                    );
                    assert_eq!((frame.width, frame.height), (width, height));
                    assert_eq!(frame.payload, frame.rgba);
                    for y in 0..height {
                        for x in 0..width {
                            let (sx, sy) = match direction {
                                0 => (x, y),
                                1 => (y, logical.1 - 1 - x),
                                2 => (logical.0 - 1 - x, logical.1 - 1 - y),
                                _ => (logical.0 - 1 - y, x),
                            };
                            let source = ((sy * logical.0 + sx) * 4) as usize;
                            let dest = ((y * width + x) * 4) as usize;
                            assert_eq!(
                                &frame.rgba[dest..dest + 4],
                                &base.rgba[source..source + 4],
                                "direction={direction}, pixel=({x},{y}), size=({width},{height})"
                            );
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn rotated_wire_formats_match_preview_pixels() {
        let scene = quadrant_scene();
        for direction in 0..=3 {
            let raw = render_orientation(
                &scene,
                (64, 64),
                Fit::Cover,
                direction,
                FrameFormat::Rgb565Le,
            );
            assert_eq!(raw.payload, rgb565(&raw.rgba));
            let jpeg =
                render_orientation(&scene, (64, 64), Fit::Cover, direction, FrameFormat::Jpeg);
            let decoded = Image::from_encoded(Data::new_copy(&jpeg.payload)).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (64, 64));
            let info = ImageInfo::new((64, 64), ColorType::RGBA8888, AlphaType::Unpremul, None);
            let mut pixels = vec![0u8; 64 * 64 * 4];
            assert!(decoded.read_pixels(
                &info,
                &mut pixels,
                64 * 4,
                (0, 0),
                sk::image::CachingHint::Allow
            ));
            for (x, y) in [(16, 16), (48, 16), (16, 48), (48, 48)] {
                let i = (y * 64 + x) * 4;
                for c in 0..3 {
                    assert!(pixels[i + c].abs_diff(jpeg.rgba[i + c]) < 15);
                }
            }
        }
    }
}
