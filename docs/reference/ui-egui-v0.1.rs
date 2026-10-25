use crate::{
    ipc::{self, Request, Response},
    runtime,
};
use anyhow::Result;
use base64::Engine;
use eframe::egui::{self, Color32, Key, Pos2, Rect, Sense, Stroke, TextureHandle, Vec2};
use pixelferry_core::*;
use pixelferry_render::Renderer;
use pixelferry_storage::{self as storage, Storage};
use std::{
    collections::BTreeSet,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

pub fn run(store: Storage, simulate: bool) -> Result<()> {
    let Some(_instance) = crate::instance::acquire(&store.root)? else {
        return Ok(());
    };
    runtime::ensure_runtime(&store, simulate)?;
    let state = store.load()?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PixelFerry · 像素渡轮")
            .with_inner_size([1460., 960.])
            .with_min_inner_size([1120., 760.])
            .with_visible(!std::env::args().any(|a| a == "--minimized"))
            .with_icon(egui::IconData {
                rgba: icon(64),
                width: 64,
                height: 64,
            }),
        ..Default::default()
    };
    eframe::run_native(
        "PixelFerry",
        options,
        Box::new(move |cc| {
            configure(&cc.egui_ctx);
            Ok(Box::new(App::new(store, state, simulate)?))
        }),
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))
}
fn icon(size: usize) -> Vec<u8> {
    let mut rgba = vec![0; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let rgb = if y > size * 3 / 4 {
                [61, 137, 158]
            } else if y > size / 2 && y < size * 3 / 4 && x > size / 8 && x < size * 7 / 8 {
                [137, 228, 204]
            } else if y > size / 4 && y < size / 2 && x > size / 3 && x < size * 2 / 3 {
                [255, 214, 159]
            } else {
                [22, 31, 46]
            };
            let i = (y * size + x) * 4;
            rgba[i..i + 3].copy_from_slice(&rgb);
            rgba[i + 3] = 255;
        }
    }
    rgba
}
fn configure(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(bytes) = std::fs::read("C:/Windows/Fonts/msyh.ttc") {
        fonts
            .font_data
            .insert("chinese".into(), egui::FontData::from_owned(bytes).into());
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, "chinese".into());
        }
    }
    ctx.set_fonts(fonts);
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = Color32::from_rgb(19, 27, 40);
    style.visuals.selection.bg_fill = Color32::from_rgb(42, 111, 109);
    style.spacing.item_spacing = Vec2::new(9., 8.);
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(14.));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(22.));
    ctx.set_style(style);
}
enum Drag {
    Move {
        start: Pos2,
        layers: Vec<Layer>,
    },
    Resize {
        before: Layer,
        anchor: (f32, f32),
    },
    Rotate {
        before: Layer,
    },
    Marquee {
        start: Pos2,
        previous: BTreeSet<String>,
    },
}
enum JobResult {
    State(AppState, String, bool),
    Asset(Asset),
    Error(String),
}
struct App {
    store: Storage,
    state: AppState,
    history: History,
    selected: BTreeSet<String>,
    response: Response,
    requests: mpsc::SyncSender<Request>,
    responses: mpsc::Receiver<(bool, Result<Response, String>)>,
    saves_pending: usize,
    history_action: bool,
    preview_target: Arc<Mutex<Option<(DeviceId, ScreenId)>>>,
    renderer: Renderer,
    texture: Option<TextureHandle>,
    last_render: Instant,
    dirty: Option<Instant>,
    notice: String,
    drag: Option<Drag>,
    guides: (Option<f32>, Option<f32>),
    selected_screen: Option<(DeviceId, ScreenId)>,
    show_output: bool,
    show_deepseek: bool,
    key: String,
    holidays: String,
    years: String,
    job: Option<mpsc::Receiver<JobResult>>,
    export_all: bool,
    simulate: bool,
    quit: bool,
    tray: Option<tray_icon::TrayIcon>,
    open_id: tray_icon::menu::MenuId,
    quit_id: tray_icon::menu::MenuId,
    last_preview: u64,
}
impl App {
    fn new(store: Storage, state: AppState, simulate: bool) -> Result<Self> {
        let (requests, rx) = mpsc::sync_channel(32);
        let (tx, responses) = mpsc::sync_channel(2);
        let root = store.root.clone();
        let preview_target = Arc::new(Mutex::new(None));
        let target = preview_target.clone();
        thread::spawn(move || loop {
            let request = match rx.recv_timeout(Duration::from_millis(150)) {
                Ok(r) => r,
                Err(mpsc::RecvTimeoutError::Timeout) => Request::Snapshot {
                    preview: target.lock().unwrap().clone(),
                },
                Err(_) => break,
            };
            let quit = matches!(request, Request::Quit);
            let saving = matches!(request, Request::SetState(_));
            let result = ipc::request(&root, request).map_err(|e| e.to_string());
            if saving || result.is_err() {
                // The window can close after queuing Save then Quit. Drain the
                // requests even if its response receiver has already gone away.
                let _ = tx.send((saving, result));
            } else {
                let _ = tx.try_send((false, result));
            }
            if quit {
                break;
            }
        });
        let menu = tray_icon::menu::Menu::new();
        let open = tray_icon::menu::MenuItem::new("打开 PixelFerry", true, None);
        let quit = tray_icon::menu::MenuItem::new("完全退出", true, None);
        menu.append_items(&[&open, &quit])?;
        let tray = tray_icon::TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("PixelFerry · 像素渡轮")
            .with_icon(tray_icon::Icon::from_rgba(icon(32), 32, 32)?)
            .build()
            .ok();
        Ok(Self {
            renderer: Renderer::new(&store.media(), &storage::default_tools()),
            holidays: state.deepseek.holidays.join("\n"),
            years: state
                .deepseek
                .calendar_years
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(","),
            store,
            state,
            history: History::default(),
            selected: BTreeSet::new(),
            response: Response::default(),
            requests,
            responses,
            saves_pending: 0,
            history_action: false,
            preview_target,
            texture: None,
            last_render: Instant::now() - Duration::from_secs(1),
            dirty: None,
            notice: "欢迎登船。选择场景，把画面送到屏幕。".into(),
            drag: None,
            guides: (None, None),
            selected_screen: None,
            show_output: false,
            show_deepseek: false,
            key: String::new(),
            job: None,
            export_all: false,
            simulate,
            quit: false,
            tray,
            open_id: open.id().clone(),
            quit_id: quit.id().clone(),
            last_preview: u64::MAX,
        })
    }
    fn send(&mut self, r: Request) {
        if let Err(e) = self.requests.try_send(r) {
            self.notice = format!("后台请求繁忙：{e}");
        }
    }
    fn changed(&mut self) {
        self.dirty = Some(Instant::now());
        self.last_render = Instant::now() - Duration::from_secs(2);
    }
    fn save(&mut self) {
        match self.state.validate() {
            Ok(()) => {
                if self
                    .requests
                    .try_send(Request::SetState(self.state.clone()))
                    .is_ok()
                {
                    self.saves_pending += 1;
                    self.dirty = None;
                } else {
                    self.notice = "保存队列繁忙，稍后自动重试".into();
                }
            }
            Err(e) => self.notice = e.to_string(),
        }
    }
    fn job(&mut self, task: impl FnOnce() -> Result<JobResult> + Send + 'static) {
        if self.job.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.job = Some(rx);
        thread::spawn(move || {
            let result = task().unwrap_or_else(|e| JobResult::Error(e.to_string()));
            let _ = tx.send(result);
        });
    }
    fn delete_layers(&mut self) {
        let selected = &self.selected;
        self.state
            .active_scene_mut()
            .layers
            .retain(|l| !selected.contains(&l.id) || l.locked);
        self.selected.clear();
    }
    fn top(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("brand").show(ctx, |ui| {
            if self.job.is_some() {
                ui.disable();
            }
            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new("PixelFerry").color(Color32::from_rgb(137, 228, 204)),
                );
                ui.label("像素渡轮 · 把画面送往不同 USB 设备");
                ui.separator();
                if ui.button("撤销").clicked() && self.history.undo(&mut self.state) {
                    self.history_action = true;
                    self.selected.clear();
                    self.changed();
                }
                if ui.button("重做").clicked() && self.history.redo(&mut self.state) {
                    self.history_action = true;
                    self.selected.clear();
                    self.changed();
                }
                if ui.button("保存").clicked() {
                    self.save();
                }
                if ui.button("账户与日历").clicked() {
                    self.show_deepseek = true;
                }
                if ui.button("完全退出").clicked() {
                    self.save();
                    self.send(Request::Quit);
                    self.quit = true;
                }
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.export_all, "导出全部场景");
                if ui
                    .add_enabled(
                        self.job.is_none() && self.dirty.is_none() && self.saves_pending == 0,
                        egui::Button::new("导出场景"),
                    )
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("PixelFerry 场景", &["pixelferry"])
                        .set_file_name("scenes.pixelferry")
                        .save_file()
                    {
                        let store = self.store.clone();
                        let state = self.state.clone();
                        let all = self.export_all;
                        self.job(move || {
                            storage::packages::export(&store, &state, &path, all)?;
                            Ok(JobResult::State(state, "场景包已导出".into(), false))
                        });
                    }
                }
                if ui
                    .add_enabled(
                        self.job.is_none() && self.dirty.is_none() && self.saves_pending == 0,
                        egui::Button::new("导入场景"),
                    )
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("场景包", &["pixelferry", "lg600", "zip"])
                        .pick_file()
                    {
                        let store = self.store.clone();
                        let state = self.state.clone();
                        self.job(move || {
                            Ok(JobResult::State(
                                storage::packages::import(&store, &state, &path)?,
                                "场景已导入".into(),
                                true,
                            ))
                        });
                    }
                }
                if ui
                    .add_enabled(
                        self.job.is_none() && self.dirty.is_none() && self.saves_pending == 0,
                        egui::Button::new("迁移旧版数据"),
                    )
                    .clicked()
                {
                    if let Some(source) = rfd::FileDialog::new()
                        .set_title("选择包含旧 state.json 的目录")
                        .pick_folder()
                    {
                        let store = self.store.clone();
                        let state = self.state.clone();
                        self.job(move || {
                            let (next, warnings) =
                                storage::migrate_legacy(&store, &state, &source)?;
                            Ok(JobResult::State(
                                next,
                                if warnings.is_empty() {
                                    "旧数据已迁移，首次绑定将应用旧设备设置".into()
                                } else {
                                    warnings.join("；")
                                },
                                true,
                            ))
                        });
                    }
                }
                if self.simulate {
                    ui.colored_label(Color32::YELLOW, "模拟模式 · 不写 USB");
                }
                if self.job.is_some() {
                    ui.spinner();
                }
            });
        });
    }
    fn left(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("library")
            .default_width(235.)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("设备与屏幕");
                    if self.response.devices.is_empty() {
                        ui.label("插入 USB 设备后自动识别");
                    }
                    for d in self.response.devices.clone() {
                        ui.collapsing(
                            format!("{} · {}", d.name, d.serial.as_deref().unwrap_or(&d.id.0)),
                            |ui| {
                                ui.label(
                                    self.response
                                        .statuses
                                        .iter()
                                        .find(|s| s.device_id == d.id)
                                        .map(|s| format!("{} · {:.1} FPS", s.message, s.fps))
                                        .unwrap_or("等待绑定场景".into()),
                                );
                                for s in &d.screens {
                                    let key = (d.id.clone(), s.id.clone());
                                    if ui
                                        .selectable_label(
                                            self.selected_screen.as_ref() == Some(&key),
                                            format!("{} {}×{}", s.name, s.width, s.height),
                                        )
                                        .clicked()
                                    {
                                        self.selected_screen = Some(key.clone());
                                        *self.preview_target.lock().unwrap() = Some(key);
                                        self.show_output = true;
                                        self.texture = None;
                                    }
                                    if !self
                                        .state
                                        .bindings
                                        .iter()
                                        .any(|b| b.device_id == d.id && b.screen_id == s.id)
                                        && ui
                                            .button(if d.requires_confirmation {
                                                "确认设备并绑定当前场景"
                                            } else {
                                                "绑定当前场景"
                                            })
                                            .clicked()
                                    {
                                        self.state.bindings.push(OutputBinding {
                                            device_id: d.id.clone(),
                                            screen_id: s.id.clone(),
                                            scene_id: self.state.active_scene_id.clone(),
                                            fit: Fit::Contain,
                                            settings: self.state.settings.clone(),
                                        });
                                    }
                                }
                                if ui.button("重新连接").clicked() {
                                    self.send(Request::Reconnect(d.id.clone()));
                                }
                            },
                        );
                    }
                    ui.separator();
                    ui.heading("场景");
                    for scene in self.state.scenes.clone() {
                        if ui
                            .selectable_label(scene.id == self.state.active_scene_id, &scene.name)
                            .clicked()
                        {
                            self.state.active_scene_id = scene.id;
                            self.selected.clear();
                            self.show_output = false;
                            self.texture = None;
                        }
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                self.state.scenes.len() < MAX_SCENES,
                                egui::Button::new("新建"),
                            )
                            .clicked()
                        {
                            let s = self.state.active_scene();
                            let scene = Scene::blank("新场景", s.width, s.height);
                            self.state.active_scene_id = scene.id.clone();
                            self.state.scenes.push(scene);
                        }
                        if ui
                            .add_enabled(
                                self.state.scenes.len() < MAX_SCENES,
                                egui::Button::new("复制"),
                            )
                            .clicked()
                        {
                            let mut s = self.state.active_scene().clone();
                            s.id = SceneId(new_id());
                            s.name.push_str(" 副本");
                            for l in &mut s.layers {
                                l.id = new_id();
                            }
                            self.state.active_scene_id = s.id.clone();
                            self.state.scenes.push(s);
                        }
                        if ui
                            .add_enabled(self.state.scenes.len() > 1, egui::Button::new("删除"))
                            .clicked()
                        {
                            let id = self.state.active_scene_id.clone();
                            self.state.scenes.retain(|s| s.id != id);
                            self.state.active_scene_id = self.state.scenes[0].id.clone();
                            for b in &mut self.state.bindings {
                                if b.scene_id == id {
                                    b.scene_id = self.state.active_scene_id.clone();
                                }
                            }
                            self.selected.clear();
                        }
                    });
                    ui.separator();
                    ui.heading("添加组件");
                    ui.horizontal_wrapped(|ui| {
                        for kind in LayerKind::ALL {
                            if ui.button(kind.label()).clicked() {
                                let l = Layer::new(kind);
                                self.selected = BTreeSet::from([l.id.clone()]);
                                if self.state.active_scene().layers.len() < 128 {
                                    self.state.active_scene_mut().layers.push(l);
                                }
                            }
                        }
                    });
                    ui.separator();
                    ui.heading("素材库");
                    if ui
                        .add_enabled(
                            self.job.is_none(),
                            egui::Button::new("导入图片 / GIF / 视频"),
                        )
                        .clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter(
                                "媒体",
                                &[
                                    "png", "jpg", "jpeg", "webp", "bmp", "avif", "gif", "mp4",
                                    "mov", "mkv", "webm", "avi",
                                ],
                            )
                            .pick_file()
                        {
                            let media = self.store.media();
                            let tools = storage::default_tools();
                            self.job(move || {
                                Ok(JobResult::Asset(storage::media::import(
                                    &path, &media, &tools,
                                )?))
                            });
                        }
                    }
                    for a in self.state.assets.clone() {
                        if ui.button(format!("＋ {}", a.name)).clicked() {
                            let kind = if a.kind == "video" {
                                LayerKind::Video
                            } else {
                                LayerKind::Image
                            };
                            if kind == LayerKind::Video
                                && self
                                    .state
                                    .active_scene()
                                    .layers
                                    .iter()
                                    .any(|l| l.kind == kind && l.visible)
                            {
                                self.notice = "每场景最多一个可见动态图层".into();
                                continue;
                            }
                            let mut l = Layer::new(kind);
                            l.asset_id = Some(a.id);
                            l.name = a.name;
                            self.selected = BTreeSet::from([l.id.clone()]);
                            self.state.active_scene_mut().layers.push(l);
                        }
                    }
                    ui.separator();
                    ui.heading("图层");
                    for l in self.state.active_scene().layers.clone().iter().rev() {
                        ui.horizontal(|ui| {
                            let mut visible = l.visible;
                            if ui.checkbox(&mut visible, "").changed() {
                                if let Some(layer) = self
                                    .state
                                    .active_scene_mut()
                                    .layers
                                    .iter_mut()
                                    .find(|x| x.id == l.id)
                                {
                                    layer.visible = visible;
                                }
                            }
                            if ui
                                .selectable_label(
                                    self.selected.contains(&l.id),
                                    format!("{}{}", if l.locked { "锁定 · " } else { "" }, l.name),
                                )
                                .clicked()
                            {
                                if !ui.input(|i| i.modifiers.shift || i.modifiers.command) {
                                    self.selected.clear();
                                }
                                if !self.selected.insert(l.id.clone()) {
                                    self.selected.remove(&l.id);
                                }
                            }
                        });
                    }
                });
            });
    }
    fn right(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("properties")
            .default_width(285.)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("画布");
                    let s = self.state.active_scene_mut();
                    ui.text_edit_singleline(&mut s.name);
                    ui.horizontal(|ui| {
                        ui.label("宽 / 高");
                        ui.add(egui::DragValue::new(&mut s.width).range(16..=4096));
                        ui.add(egui::DragValue::new(&mut s.height).range(16..=4096));
                    });
                    color_edit(ui, "背景", &mut s.background);
                    ui.separator();
                    if self.selected.len() > 1 {
                        self.multi(ui);
                    } else if let Some(id) = self.selected.first().cloned() {
                        let sensors = self.response.data.sensors.clone();
                        let assets = self.state.assets.clone();
                        if let Some(l) = self
                            .state
                            .active_scene_mut()
                            .layers
                            .iter_mut()
                            .find(|l| l.id == id)
                        {
                            ui.heading("组件属性");
                            ui.text_edit_singleline(&mut l.name);
                            ui.checkbox(&mut l.locked, "锁定");
                            ui.checkbox(&mut l.visible, "可见");
                            number(ui, "X", &mut l.x, -16384. ..=16384.);
                            number(ui, "Y", &mut l.y, -16384. ..=16384.);
                            number(ui, "宽", &mut l.width, 4. ..=8192.);
                            number(ui, "高", &mut l.height, 4. ..=8192.);
                            number(ui, "旋转", &mut l.rotation, -360. ..=360.);
                            ui.add(egui::Slider::new(&mut l.opacity, 0. ..=1.).text("不透明度"));
                            color_edit(ui, "颜色", &mut l.color);
                            if matches!(
                                l.kind,
                                LayerKind::Text
                                    | LayerKind::Clock
                                    | LayerKind::Sensor
                                    | LayerKind::Deepseek
                            ) {
                                ui.text_edit_multiline(&mut l.text);
                                number(ui, "字号", &mut l.font_size, 8. ..=240.);
                                ui.text_edit_singleline(&mut l.font_family);
                                ui.checkbox(&mut l.bold, "粗体");
                                combo(
                                    ui,
                                    "对齐",
                                    &mut l.align,
                                    &[("left", "左对齐"), ("center", "居中"), ("right", "右对齐")],
                                );
                            }
                            if matches!(l.kind, LayerKind::Sensor | LayerKind::Bar) {
                                egui::ComboBox::from_id_salt("sensor")
                                    .selected_text(
                                        l.sensor_id
                                            .as_ref()
                                            .and_then(|id| sensors.iter().find(|s| &s.id == id))
                                            .map(|s| s.name.clone())
                                            .unwrap_or("选择传感器".into()),
                                    )
                                    .show_ui(ui, |ui| {
                                        for s in &sensors {
                                            ui.selectable_value(
                                                &mut l.sensor_id,
                                                Some(s.id.clone()),
                                                format!("{} {}", s.name, s.device),
                                            );
                                        }
                                    });
                            }
                            if matches!(l.kind, LayerKind::Image | LayerKind::Video) {
                                egui::ComboBox::from_id_salt("asset")
                                    .selected_text(l.asset_id.as_deref().unwrap_or("选择素材"))
                                    .show_ui(ui, |ui| {
                                        for a in assets.iter().filter(|a| {
                                            a.kind
                                                == if l.kind == LayerKind::Video {
                                                    "video"
                                                } else {
                                                    "image"
                                                }
                                        }) {
                                            ui.selectable_value(
                                                &mut l.asset_id,
                                                Some(a.id.clone()),
                                                &a.name,
                                            );
                                        }
                                    });
                                fit_edit(ui, &mut l.fit);
                            }
                            if l.kind == LayerKind::Bar {
                                color_edit(ui, "底色", &mut l.bar_background);
                                ui.horizontal(|ui| {
                                    ui.label("最小 / 最大");
                                    ui.add(egui::DragValue::new(&mut l.min));
                                    ui.add(egui::DragValue::new(&mut l.max));
                                });
                                combo(
                                    ui,
                                    "进度来源",
                                    &mut l.bar_source,
                                    &[
                                        ("sensor", "传感器"),
                                        ("deepseek-period", "DeepSeek 峰谷进度"),
                                    ],
                                );
                                combo(
                                    ui,
                                    "进度方向",
                                    &mut l.bar_direction,
                                    &[("elapsed", "已过"), ("remaining", "剩余")],
                                );
                            }
                            if l.kind == LayerKind::Deepseek {
                                combo(
                                    ui,
                                    "指标",
                                    &mut l.deepseek_metric,
                                    &[
                                        ("summary", "综合信息"),
                                        ("balance", "余额"),
                                        ("today", "今日估算"),
                                        ("total", "累计估算"),
                                        ("period", "峰谷时段"),
                                    ],
                                );
                            }
                            if matches!(l.kind, LayerKind::Rect | LayerKind::Circle) {
                                if l.kind == LayerKind::Rect {
                                    number(ui, "圆角", &mut l.corner_radius, 0. ..=1000.);
                                }
                                ui.checkbox(&mut l.glass, "毛玻璃");
                                if l.glass {
                                    ui.add(
                                        egui::Slider::new(&mut l.glass_blur, 0. ..=40.)
                                            .text("背景模糊"),
                                    );
                                    ui.add(
                                        egui::Slider::new(&mut l.glass_tint_opacity, 0. ..=1.)
                                            .text("着色强度"),
                                    );
                                }
                            }
                        }
                        ui.horizontal(|ui| {
                            for (label, up) in [("上移", true), ("下移", false)] {
                                if ui.button(label).clicked() {
                                    let ls = &mut self.state.active_scene_mut().layers;
                                    if let Some(i) = ls.iter().position(|l| l.id == id) {
                                        if up && i + 1 < ls.len() {
                                            ls.swap(i, i + 1);
                                        } else if !up && i > 0 {
                                            ls.swap(i, i - 1);
                                        }
                                    }
                                }
                            }
                            if ui.button("删除").clicked() {
                                self.delete_layers();
                            }
                        });
                    } else {
                        ui.label("选择组件以编辑属性");
                    }
                    ui.separator();
                    self.output(ui);
                });
            });
    }
    fn multi(&mut self, ui: &mut egui::Ui) {
        ui.heading(format!("已选 {} 个组件", self.selected.len()));
        for (label, axis, position) in [
            ("左对齐", 0, 0),
            ("水平中心", 0, 1),
            ("右对齐", 0, 2),
            ("顶对齐", 1, 0),
            ("垂直中心", 1, 1),
            ("底对齐", 1, 2),
            ("画布居中", 2, 0),
        ] {
            if ui.button(label).clicked() {
                let selected = &self.selected;
                let scene = self.state.active_scene_mut();
                let boxes: Vec<_> = scene
                    .layers
                    .iter()
                    .filter(|l| selected.contains(&l.id) && !l.locked)
                    .map(layer_bounds)
                    .collect();
                let bounds = union_bounds(&boxes);
                for l in scene
                    .layers
                    .iter_mut()
                    .filter(|l| selected.contains(&l.id) && !l.locked)
                {
                    let b = layer_bounds(l);
                    match axis {
                        0 => {
                            l.x += bounds.x + (bounds.width - b.width) * position as f32 / 2. - b.x
                        }
                        1 => {
                            l.y +=
                                bounds.y + (bounds.height - b.height) * position as f32 / 2. - b.y
                        }
                        _ => {
                            l.x += (scene.width as f32 - bounds.width) / 2. - bounds.x;
                            l.y += (scene.height as f32 - bounds.height) / 2. - bounds.y;
                        }
                    }
                }
            }
        }
        if ui.button("删除选中").clicked() {
            self.delete_layers();
        }
    }
    fn output(&mut self, ui: &mut egui::Ui) {
        ui.heading("屏幕输出");
        let Some((device, screen)) = &self.selected_screen else {
            ui.label("在左侧选择设备屏幕");
            return;
        };
        let scenes = self.state.scenes.clone();
        let caps = self
            .response
            .devices
            .iter()
            .find(|d| &d.id == device)
            .and_then(|d| d.screens.iter().find(|s| &s.id == screen))
            .cloned();
        let mut remove = false;
        let jpeg = caps
            .as_ref()
            .is_some_and(|c| c.encoding == FrameFormat::Jpeg);
        if let Some(b) = self
            .state
            .bindings
            .iter_mut()
            .find(|b| &b.device_id == device && &b.screen_id == screen)
        {
            egui::ComboBox::from_id_salt("output-scene")
                .selected_text(
                    scenes
                        .iter()
                        .find(|s| s.id == b.scene_id)
                        .map(|s| s.name.as_str())
                        .unwrap_or("选择场景"),
                )
                .show_ui(ui, |ui| {
                    for s in &scenes {
                        ui.selectable_value(&mut b.scene_id, s.id.clone(), &s.name);
                    }
                });
            fit_edit(ui, &mut b.fit);
            if let Some(c) = caps {
                if c.power {
                    ui.checkbox(&mut b.settings.power, "屏幕开启");
                }
                if c.brightness {
                    ui.add(egui::Slider::new(&mut b.settings.brightness, 0..=100).text("亮度"));
                }
                if c.direction {
                    egui::ComboBox::from_id_salt("direction")
                        .selected_text(format!("安装方向 {}°", b.settings.direction as u32 * 90))
                        .show_ui(ui, |ui| {
                            for d in 0..4 {
                                ui.selectable_value(
                                    &mut b.settings.direction,
                                    d,
                                    format!("{}°", d as u32 * 90),
                                );
                            }
                        });
                }
            }
            if jpeg {
                ui.add(egui::Slider::new(&mut b.settings.quality, 40..=90).text("JPEG 质量"));
            }
            ui.add(egui::Slider::new(&mut b.settings.render_fps, 1..=30).text("渲染 FPS"));
            remove = ui.button("解除绑定").clicked();
        }
        if remove {
            self.state
                .bindings
                .retain(|b| &b.device_id != device || &b.screen_id != screen);
        }
        ui.separator();
        ui.checkbox(&mut self.state.settings.auto_start, "登录 Windows 后启动");
    }
    fn canvas(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.selectable_label(!self.show_output, "编辑场景").clicked() {
                    self.show_output = false;
                    self.texture = None;
                }
                if ui
                    .add_enabled(
                        self.selected_screen.is_some(),
                        egui::Button::new("设备画面").selected(self.show_output),
                    )
                    .clicked()
                {
                    self.show_output = true;
                    self.texture = None;
                }
            });
            let scene = self.state.active_scene().clone();
            let mut size = (scene.width, scene.height);
            if self.show_output {
                if let Some(p) = self.response.preview.clone() {
                    size = (p.width, p.height);
                    if p.sequence != self.last_preview || self.texture.is_none() {
                        if let Ok(rgba) = base64::engine::general_purpose::STANDARD.decode(&p.rgba)
                        {
                            if rgba.len() == p.width as usize * p.height as usize * 4 {
                                self.set_texture(ctx, size, &rgba);
                                self.last_preview = p.sequence;
                            }
                        }
                    }
                }
            } else if self.texture.is_none()
                || self.last_render.elapsed() > Duration::from_millis(33)
            {
                match self.renderer.render(pixelferry_render::RenderRequest {
                    scene: &scene,
                    assets: &self.state.assets,
                    data: &self.response.data,
                    prefs: &self.state.deepseek,
                    size: (scene.width, scene.height),
                    fit: Fit::Contain,
                    quality: 70,
                    format: FrameFormat::Rgba8,
                    now: pixelferry_services::now(),
                }) {
                    Ok(f) => self.set_texture(ctx, (f.width, f.height), &f.rgba),
                    Err(e) => self.notice = e.to_string(),
                }
                self.last_render = Instant::now();
            }
            let available = ui.available_size() - Vec2::splat(30.);
            let scale = (available.x / size.0 as f32)
                .min(available.y / size.1 as f32)
                .clamp(0.05, 2.);
            let (canvas, response) = ui.allocate_exact_size(
                Vec2::new(size.0 as f32 * scale, size.1 as f32 * scale),
                Sense::click_and_drag(),
            );
            ui.painter().rect_filled(canvas, 8., Color32::BLACK);
            if let Some(t) = &self.texture {
                ui.painter().image(
                    t.id(),
                    canvas,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                    Color32::WHITE,
                );
            }
            if !self.show_output {
                self.interact(ui, canvas, &response, scale);
            }
            if self.show_output && self.response.preview.is_none() {
                ui.painter().text(
                    canvas.center(),
                    egui::Align2::CENTER_CENTER,
                    "等待设备画面",
                    egui::FontId::proportional(22.),
                    Color32::GRAY,
                );
            }
        });
    }
    fn set_texture(&mut self, ctx: &egui::Context, size: (u32, u32), rgba: &[u8]) {
        let image =
            egui::ColorImage::from_rgba_unmultiplied([size.0 as usize, size.1 as usize], rgba);
        if let Some(t) = &mut self.texture {
            t.set(image, egui::TextureOptions::LINEAR)
        } else {
            self.texture = Some(ctx.load_texture("scene", image, egui::TextureOptions::LINEAR));
        }
    }
    fn interact(&mut self, ui: &mut egui::Ui, canvas: Rect, response: &egui::Response, scale: f32) {
        let scene = self.state.active_scene().clone();
        let to_local =
            |p: Pos2| Pos2::new((p.x - canvas.min.x) / scale, (p.y - canvas.min.y) / scale);
        let to_screen = |p: Pos2| canvas.min + Vec2::new(p.x * scale, p.y * scale);
        let mut handles = vec![];
        for l in scene
            .layers
            .iter()
            .filter(|l| self.selected.contains(&l.id))
        {
            let points = [
                world(l, 0., 0.),
                world(l, l.width, 0.),
                world(l, l.width, l.height),
                world(l, 0., l.height),
            ]
            .map(to_screen);
            ui.painter().add(egui::Shape::closed_line(
                points.to_vec(),
                Stroke::new(1.5_f32, Color32::from_rgb(137, 228, 204)),
            ));
            if self.selected.len() == 1 && !l.locked {
                for (ax, ay) in [
                    (0., 0.),
                    (0.5, 0.),
                    (1., 0.),
                    (0., 0.5),
                    (1., 0.5),
                    (0., 1.),
                    (0.5, 1.),
                    (1., 1.),
                ] {
                    let p = to_screen(world(l, ax * l.width, ay * l.height));
                    let r = Rect::from_center_size(p, Vec2::splat(9.));
                    ui.painter().rect_filled(r, 2., Color32::LIGHT_GREEN);
                    handles.push((
                        r,
                        Drag::Resize {
                            before: l.clone(),
                            anchor: (ax, ay),
                        },
                    ));
                }
                let p = to_screen(world(l, l.width / 2., -25. / scale));
                ui.painter().circle_filled(p, 5., Color32::LIGHT_YELLOW);
                handles.push((
                    Rect::from_center_size(p, Vec2::splat(15.)),
                    Drag::Rotate { before: l.clone() },
                ));
            }
        }
        let additive = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        if response.drag_started() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let p = to_local(pointer);
                if let Some((_, drag)) = handles.into_iter().find(|(r, _)| r.contains(pointer)) {
                    self.history.record(&self.state);
                    self.drag = Some(drag);
                } else if let Some(hit) = scene
                    .layers
                    .iter()
                    .rev()
                    .find(|l| l.visible && !l.locked && hit(l, p))
                {
                    if !self.selected.contains(&hit.id) {
                        if !additive {
                            self.selected.clear();
                        }
                        self.selected.insert(hit.id.clone());
                    }
                    self.history.record(&self.state);
                    self.drag = Some(Drag::Move {
                        start: p,
                        layers: scene
                            .layers
                            .iter()
                            .filter(|l| self.selected.contains(&l.id) && !l.locked)
                            .cloned()
                            .collect(),
                    });
                } else {
                    let previous = if additive {
                        self.selected.clone()
                    } else {
                        BTreeSet::new()
                    };
                    self.selected = previous.clone();
                    self.drag = Some(Drag::Marquee { start: p, previous });
                }
            }
        }
        if response.clicked() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let p = to_local(pointer);
                if let Some(l) = scene
                    .layers
                    .iter()
                    .rev()
                    .find(|l| l.visible && !l.locked && hit(l, p))
                {
                    if !additive {
                        self.selected.clear();
                        self.selected.insert(l.id.clone());
                    } else if !self.selected.insert(l.id.clone()) {
                        self.selected.remove(&l.id);
                    }
                } else if !additive {
                    self.selected.clear();
                }
            }
        }
        if response.dragged() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let p = to_local(pointer);
                let alt = ui.input(|i| i.modifiers.alt);
                let others: Vec<_> = scene
                    .layers
                    .iter()
                    .filter(|l| l.visible && !self.selected.contains(&l.id))
                    .map(layer_bounds)
                    .collect();
                match &self.drag {
                    Some(Drag::Move { start, layers }) => {
                        let mut dx = p.x - start.x;
                        let mut dy = p.y - start.y;
                        let b = union_bounds(&layers.iter().map(layer_bounds).collect::<Vec<_>>());
                        if !alt {
                            let (sx, sy, v, h) = snap(
                                Bounds {
                                    x: b.x + dx,
                                    y: b.y + dy,
                                    ..b
                                },
                                &others,
                                scene.width as f32,
                                scene.height as f32,
                                5.,
                            );
                            dx += sx;
                            dy += sy;
                            self.guides = (v, h);
                        }
                        for original in layers {
                            if let Some(l) = self
                                .state
                                .active_scene_mut()
                                .layers
                                .iter_mut()
                                .find(|l| l.id == original.id)
                            {
                                l.x = (original.x + dx).round();
                                l.y = (original.y + dy).round();
                            }
                        }
                    }
                    Some(Drag::Resize { before, anchor }) => {
                        let before = before.clone();
                        let anchor = *anchor;
                        let mut p = p;
                        if !alt {
                            let (dx, dy, v, h) = snap(
                                Bounds {
                                    x: p.x,
                                    y: p.y,
                                    width: 0.,
                                    height: 0.,
                                },
                                &others,
                                scene.width as f32,
                                scene.height as f32,
                                5.,
                            );
                            p += Vec2::new(dx, dy);
                            self.guides = (v, h);
                        }
                        if let Some(l) = self
                            .state
                            .active_scene_mut()
                            .layers
                            .iter_mut()
                            .find(|l| l.id == before.id)
                        {
                            *l = resized(&before, anchor, p, alt);
                        }
                    }
                    Some(Drag::Rotate { before }) => {
                        let before = before.clone();
                        let center = world(&before, before.width / 2., before.height / 2.);
                        let mut angle = (p.y - center.y).atan2(p.x - center.x).to_degrees() + 90.;
                        if ui.input(|i| i.modifiers.shift) {
                            angle = (angle / 15.).round() * 15.;
                        }
                        angle = angle.clamp(-360., 360.);
                        let (s, c) = angle.to_radians().sin_cos();
                        if let Some(l) = self
                            .state
                            .active_scene_mut()
                            .layers
                            .iter_mut()
                            .find(|l| l.id == before.id)
                        {
                            l.rotation = angle;
                            l.x = center.x - before.width / 2. * c + before.height / 2. * s;
                            l.y = center.y - before.width / 2. * s - before.height / 2. * c;
                        }
                    }
                    Some(Drag::Marquee { start, previous }) => {
                        let r = Rect::from_two_pos(*start, p);
                        let b = Bounds {
                            x: r.min.x,
                            y: r.min.y,
                            width: r.width(),
                            height: r.height(),
                        };
                        self.selected = previous.clone();
                        self.selected.extend(
                            scene
                                .layers
                                .iter()
                                .filter(|l| l.visible && !l.locked && b.intersects(layer_bounds(l)))
                                .map(|l| l.id.clone()),
                        );
                        ui.painter().rect_stroke(
                            Rect::from_min_max(to_screen(r.min), to_screen(r.max)),
                            0.,
                            Stroke::new(1_f32, Color32::LIGHT_GREEN),
                            egui::StrokeKind::Inside,
                        );
                    }
                    None => {}
                }
            }
        }
        if response.drag_stopped() {
            self.drag = None;
            self.guides = (None, None);
        }
        if let Some(x) = self.guides.0 {
            ui.painter().line_segment(
                [
                    to_screen(Pos2::new(x, 0.)),
                    to_screen(Pos2::new(x, scene.height as f32)),
                ],
                Stroke::new(1_f32, Color32::LIGHT_GREEN),
            );
        }
        if let Some(y) = self.guides.1 {
            ui.painter().line_segment(
                [
                    to_screen(Pos2::new(0., y)),
                    to_screen(Pos2::new(scene.width as f32, y)),
                ],
                Stroke::new(1_f32, Color32::LIGHT_GREEN),
            );
        }
    }
    fn account(&mut self, ctx: &egui::Context) {
        if !self.show_deepseek {
            return;
        }
        let mut open = true;
        egui::Window::new("DeepSeek · 账户与日历")
            .open(&mut open)
            .default_width(480.)
            .show(ctx, |ui| {
                ui.label(format!("连接状态：{}", self.response.data.deepseek.status));
                if let Some(e) = &self.response.data.deepseek.error {
                    ui.colored_label(Color32::YELLOW, e);
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.key)
                        .password(true)
                        .hint_text("API Key（Windows 加密保存）"),
                );
                ui.horizontal(|ui| {
                    if ui.button("保存并连接").clicked() {
                        let key = std::mem::take(&mut self.key);
                        self.send(Request::SetKey(key));
                    }
                    if ui.button("移除 Key").clicked() {
                        self.key.clear();
                        self.send(Request::ClearKey);
                    }
                    if ui.button("刷新").clicked() {
                        self.send(Request::Refresh);
                    }
                });
                ui.label("消费为本地估算，不等同官方账单。");
                combo(
                    ui,
                    "币种",
                    &mut self.state.deepseek.currency,
                    &[("CNY", "CNY"), ("USD", "USD")],
                );
                ui.label(pixelferry_services::deepseek::text(
                    "summary",
                    &self.response.data.deepseek,
                    &self.state.deepseek,
                    pixelferry_services::now(),
                ));
                ui.separator();
                ui.label("节假日：每行 YYYY-MM-DD");
                ui.add(egui::TextEdit::multiline(&mut self.holidays).desired_rows(7));
                ui.label("已核对年份：逗号分隔");
                ui.text_edit_singleline(&mut self.years);
                if ui.button("保存日历").clicked() {
                    let holidays: Vec<_> = self
                        .holidays
                        .lines()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect();
                    let years = self
                        .years
                        .split(',')
                        .map(|s| s.trim().parse::<i32>())
                        .collect::<Result<Vec<_>, _>>();
                    if holidays.len() <= 1500
                        && holidays
                            .iter()
                            .all(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok())
                        && years.as_ref().is_ok_and(|ys| {
                            ys.len() <= 75 && ys.iter().all(|y| (2026..=2100).contains(y))
                        })
                    {
                        self.state.deepseek.holidays = holidays;
                        self.state.deepseek.calendar_years = years.unwrap();
                        self.notice = "日历已保存".into();
                    } else {
                        self.notice = "日期或年份格式无效".into();
                    }
                }
            });
        self.show_deepseek = open;
    }
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let command = ctx.input(|i| i.modifiers.command);
        let shift = ctx.input(|i| i.modifiers.shift);
        if command && ctx.input(|i| i.key_pressed(Key::Z)) {
            self.history_action = true;
            if shift {
                self.history.redo(&mut self.state);
            } else {
                self.history.undo(&mut self.state);
            }
            self.changed();
        }
        if command && ctx.input(|i| i.key_pressed(Key::S)) {
            self.save();
        }
        if command && ctx.input(|i| i.key_pressed(Key::A)) {
            self.selected = self
                .state
                .active_scene()
                .layers
                .iter()
                .filter(|l| l.visible && !l.locked)
                .map(|l| l.id.clone())
                .collect();
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.selected.clear();
        }
        if ctx.input(|i| i.key_pressed(Key::Delete)) {
            self.delete_layers();
        }
        let step = if shift { 10. } else { 1. };
        let dx = if ctx.input(|i| i.key_pressed(Key::ArrowRight)) {
            step
        } else if ctx.input(|i| i.key_pressed(Key::ArrowLeft)) {
            -step
        } else {
            0.
        };
        let dy = if ctx.input(|i| i.key_pressed(Key::ArrowDown)) {
            step
        } else if ctx.input(|i| i.key_pressed(Key::ArrowUp)) {
            -step
        } else {
            0.
        };
        if dx != 0. || dy != 0. {
            let selected = &self.selected;
            for l in &mut self.state.active_scene_mut().layers {
                if selected.contains(&l.id) && !l.locked {
                    l.x += dx;
                    l.y += dy;
                }
            }
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(33));
        self.history_action = false;
        while let Ok((saving, result)) = self.responses.try_recv() {
            if saving {
                self.saves_pending = self.saves_pending.saturating_sub(1);
            }
            match result {
                Ok(r) => self.response = r,
                Err(e) => {
                    self.notice = e;
                    if saving {
                        self.dirty = Some(Instant::now());
                    }
                }
            }
        }
        if let Some(job) = &self.job {
            if let Ok(result) = job.try_recv() {
                self.job = None;
                match result {
                    JobResult::Error(e) => self.notice = e,
                    JobResult::Asset(a) => {
                        self.history.record(&self.state);
                        self.state.assets.push(a);
                        self.changed();
                        self.notice = "素材已导入，点击素材添加到场景".into();
                    }
                    JobResult::State(state, message, changed) => {
                        if changed {
                            self.history.record(&self.state);
                            self.state = state;
                            self.holidays = self.state.deepseek.holidays.join("\n");
                            self.years = self
                                .state
                                .deepseek
                                .calendar_years
                                .iter()
                                .map(i32::to_string)
                                .collect::<Vec<_>>()
                                .join(",");
                            self.selected.clear();
                            self.changed();
                            self.send(Request::ReloadServices);
                        }
                        self.notice = message;
                    }
                }
            }
        }
        while let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            if event.id == self.open_id {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            if event.id == self.quit_id {
                if self.job.is_some() {
                    self.notice = "正在处理文件，请完成后退出".into();
                    continue;
                }
                self.save();
                self.send(Request::Quit);
                self.quit = true;
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit {
            if self.job.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.notice = "文件处理完成后可关闭窗口".into();
                return;
            }
            if self.tray.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.save();
            } else {
                self.save();
                self.send(Request::Quit);
                self.quit = true;
            }
        }
        if self.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let before = self.state.clone();
        let dragging = self.drag.is_some();
        self.top(ctx);
        if self.job.is_none() {
            self.shortcuts(ctx);
            self.left(ctx);
            self.right(ctx);
            self.canvas(ctx);
            self.account(ctx);
        } else {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("正在处理素材或场景包，后台播放继续运行…");
            });
        }
        if self.state != before {
            if let Err(e) = self.state.validate() {
                self.state = before;
                self.notice = e.to_string();
            } else {
                if !dragging && self.drag.is_none() && !self.history_action {
                    self.history.record(&before);
                }
                self.changed();
            }
        }
        if self
            .dirty
            .is_some_and(|t| t.elapsed() > Duration::from_millis(300))
            && self.drag.is_none()
            && self.job.is_none()
        {
            self.save();
        }
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.label(&self.notice);
            if let Some(n) = self.renderer.notices.last() {
                ui.colored_label(Color32::YELLOW, n);
            }
        });
    }
}
fn number(ui: &mut egui::Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).speed(1.).range(range));
    });
}
fn color_edit(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        let c = pixelferry_render::color(value);
        let mut rgb = [c.r(), c.g(), c.b()];
        if ui.color_edit_button_srgb(&mut rgb).changed() {
            *value = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
        }
    });
}
fn combo(ui: &mut egui::Ui, label: &str, value: &mut String, options: &[(&str, &str)]) {
    egui::ComboBox::from_id_salt(label)
        .selected_text(
            options
                .iter()
                .find(|(v, _)| v == value)
                .map(|(_, l)| *l)
                .unwrap_or(value),
        )
        .show_ui(ui, |ui| {
            for (v, label) in options {
                ui.selectable_value(value, (*v).into(), *label);
            }
        });
}
fn fit_edit(ui: &mut egui::Ui, fit: &mut Fit) {
    ui.horizontal(|ui| {
        ui.selectable_value(fit, Fit::Contain, "等比留边");
        ui.selectable_value(fit, Fit::Cover, "居中裁切");
    });
}
fn world(l: &Layer, x: f32, y: f32) -> Pos2 {
    let (s, c) = l.rotation.to_radians().sin_cos();
    Pos2::new(l.x + x * c - y * s, l.y + x * s + y * c)
}
fn hit(l: &Layer, p: Pos2) -> bool {
    let (s, c) = l.rotation.to_radians().sin_cos();
    let dx = p.x - l.x;
    let dy = p.y - l.y;
    let x = dx * c + dy * s;
    let y = -dx * s + dy * c;
    x >= 0. && x <= l.width && y >= 0. && y <= l.height
}
fn resized(before: &Layer, anchor: (f32, f32), pointer: Pos2, centered: bool) -> Layer {
    let (s, c) = before.rotation.to_radians().sin_cos();
    let dx = pointer.x - before.x;
    let dy = pointer.y - before.y;
    let px = dx * c + dy * s;
    let py = -dx * s + dy * c;
    let (ax, ay) = anchor;
    let (fx, fy) = if centered {
        (0.5, 0.5)
    } else {
        (1. - ax, 1. - ay)
    };
    let mut w = if ax == 0.5 {
        before.width
    } else {
        (px - before.width * fx) / (ax - fx)
    };
    let mut h = if ay == 0.5 {
        before.height
    } else {
        (py - before.height * fy) / (ay - fy)
    };
    if ax != 0.5 && ay != 0.5 {
        let ratio = (w / before.width).max(h / before.height);
        w = before.width * ratio;
        h = before.height * ratio;
    }
    w = w.clamp(4., 8192.);
    h = h.clamp(4., 8192.);
    let fixed = world(before, before.width * fx, before.height * fy);
    let mut next = before.clone();
    next.width = w;
    next.height = h;
    next.x = fixed.x - w * fx * c + h * fy * s;
    next.y = fixed.y - w * fx * s - h * fy * c;
    next
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resize_keeps_opposite_corner() {
        let l = Layer {
            rotation: 30.,
            width: 100.,
            height: 100.,
            ..Default::default()
        };
        let next = resized(&l, (1., 1.), world(&l, 200., 200.), false);
        assert!((next.width - 200.).abs() < 0.01);
        assert!((world(&next, 0., 0.) - world(&l, 0., 0.)).length() < 0.01);
    }
    #[test]
    fn centered_resize_keeps_center() {
        let l = Layer::default();
        let center = world(&l, l.width / 2., l.height / 2.);
        let next = resized(&l, (1., 0.5), world(&l, 500., 32.), true);
        assert!((world(&next, next.width / 2., next.height / 2.) - center).length() < 0.01);
    }
}
