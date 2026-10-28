use crate::ipc::{self, Preview, Request, Response};
use anyhow::{ensure, Result};
use base64::Engine;
use pixelferry_core::*;
use pixelferry_devices::{self as devices, Connection};
use pixelferry_render::{DataSnapshot, Renderer};
use pixelferry_services::{self as services, deepseek};
use pixelferry_storage::{default_tools, Storage};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Serialize, Deserialize)]
struct WorkerConfig {
    device: DeviceDescriptor,
    state: AppState,
    data: DataSnapshot,
    preview_screen: Option<ScreenId>,
}
#[derive(Serialize, Deserialize)]
enum WorkerMessage {
    Config(Box<WorkerConfig>),
    Stop,
}
#[derive(Serialize, Deserialize)]
struct WorkerEvent {
    status: DeviceStatus,
    preview: Option<(ScreenId, Preview)>,
}
struct Worker {
    _job: services::process::Job,
    child: Child,
    input: Arc<Mutex<Option<WorkerMessage>>>,
    events: mpsc::Receiver<WorkerEvent>,
    last: Instant,
    status: DeviceStatus,
    next: Instant,
    revision: u64,
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn quiet(c: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
}
fn spawn_worker(store: &Storage, device: &DeviceDescriptor) -> Result<Worker> {
    let mut c = Command::new(std::env::current_exe()?);
    quiet(&mut c);
    let mut child = c
        .arg("--worker")
        .arg(format!("--data-dir={}", store.root.display()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdin: ChildStdin = child.stdin.take().unwrap();
    let job = match services::process::Job::attach(&child) {
        Ok(job) => job,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    let stdout = child.stdout.take().unwrap();
    let input = Arc::new(Mutex::new(None));
    let writer = input.clone();
    let quit = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let quit_reader = quit.clone();
    thread::spawn(move || {
        while !quit.load(std::sync::atomic::Ordering::Relaxed) {
            let next = writer.lock().unwrap().take();
            if let Some(message) = next {
                if ipc::write(&mut stdin, &message).is_err() {
                    break;
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
    });
    let (tx, events) = mpsc::sync_channel(8);
    thread::spawn(move || {
        let mut stdout = stdout;
        while let Ok(event) = ipc::read::<WorkerEvent>(&mut stdout) {
            if tx.send(event).is_err() {
                break;
            }
        }
        quit_reader.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    Ok(Worker {
        _job: job,
        child,
        input,
        events,
        last: Instant::now(),
        status: DeviceStatus {
            device_id: device.id.clone(),
            state: "connecting".into(),
            message: "正在连接".into(),
            ..Default::default()
        },
        next: Instant::now(),
        revision: 0,
    })
}

pub fn serve(store: Storage, simulate: bool) -> Result<()> {
    let mut server = ipc::Server::new(&store.root)?;
    let _power = crate::platform::PowerSubscription::new().ok();
    let mut state = store.load()?;
    let mut devices = vec![];
    let mut workers = BTreeMap::<DeviceId, Worker>::new();
    let mut previews = BTreeMap::<(DeviceId, ScreenId), Preview>::new();
    let mut discovery = Instant::now() - Duration::from_secs(10);
    let mut data = DataSnapshot::default();
    let mut revision = 1u64;
    let mut selected_preview: Option<(DeviceId, ScreenId)> = None;
    let (service_tx, service_rx) = mpsc::channel::<ServiceCommand>();
    let (data_tx, data_rx) = mpsc::sync_channel(2);
    let (sensor_tx, sensor_rx) = mpsc::sync_channel(2);
    let service_alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let sensor_alive = service_alive.clone();
    thread::spawn(move || {
        while sensor_alive.load(std::sync::atomic::Ordering::Relaxed) {
            let result = services::sensors::read().map_err(|e| e.to_string());
            let _ = sensor_tx.try_send(result);
            thread::sleep(Duration::from_millis(1000));
        }
    });
    let services_root = store.root.clone();
    thread::spawn(move || {
        let mut service = deepseek::Service::new(&services_root);
        loop {
            while let Ok(command) = service_rx.try_recv() {
                match command {
                    ServiceCommand::Key(key, tx) => {
                        let result = service
                            .change_key(key.as_deref())
                            .map_err(|e| e.to_string());
                        let _ = tx.send(result);
                    }
                    ServiceCommand::Refresh => service.refresh(),
                    ServiceCommand::Reload => service = deepseek::Service::new(&services_root),
                    ServiceCommand::Stop => return,
                }
            }
            service.tick();
            let snapshot = DataSnapshot {
                deepseek: service.snapshot.clone(),
                ..Default::default()
            };
            if let Err(mpsc::TrySendError::Disconnected(_)) = data_tx.try_send(snapshot) {
                return;
            }
            thread::sleep(Duration::from_millis(500));
        }
    });
    let mut running = true;
    while running {
        while let Ok(snapshot) = data_rx.try_recv() {
            data.deepseek = snapshot.deepseek;
            revision += 1;
        }
        while let Ok(result) = sensor_rx.try_recv() {
            match result {
                Ok(samples) => {
                    data.sensors = samples;
                    data.sensor_error = None;
                }
                Err(e) => {
                    data.sensors.clear();
                    data.sensor_error = Some(e);
                }
            }
            revision += 1;
        }
        if discovery.elapsed() > Duration::from_secs(3) {
            match devices::discover(simulate) {
                Ok(found) => devices = found,
                Err(e) => store.log(&format!("设备枚举失败：{e}")),
            }
            discovery = Instant::now();
        }
        if crate::platform::SUSPENDED.load(std::sync::atomic::Ordering::Relaxed) {
            workers.clear();
            previews.clear();
        }
        workers.retain(|id, _| {
            devices.iter().any(|d| &d.id == id) && state.bindings.iter().any(|b| &b.device_id == id)
        });
        for device in &devices {
            if crate::platform::SUSPENDED.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            if !state.bindings.iter().any(|b| b.device_id == device.id) {
                continue;
            }
            if !workers.contains_key(&device.id) {
                if workers.len() >= 8 {
                    continue;
                }
                match spawn_worker(&store, device) {
                    Ok(w) => {
                        workers.insert(device.id.clone(), w);
                    }
                    Err(e) => {
                        store.log(&format!("设备进程启动失败：{e}"));
                        continue;
                    }
                }
            }
            let w = workers.get_mut(&device.id).unwrap();
            while let Ok(event) = w.events.try_recv() {
                w.last = Instant::now();
                w.status = event.status;
                if let Some((screen, preview)) = event.preview {
                    previews.insert((device.id.clone(), screen), preview);
                }
            }
            let exited = w.child.try_wait()?.is_some();
            if exited || w.last.elapsed() > Duration::from_secs(12) {
                let _ = w.child.kill();
                let _ = w.child.wait();
                w.status.state = "error".into();
                w.status.message = "设备工作进程超时或退出，正在恢复".into();
                if w.next.elapsed() > Duration::from_secs(2) {
                    *w = spawn_worker(&store, device)?;
                    w.next = Instant::now() + Duration::from_secs(2);
                }
                continue;
            }
            if revision != w.revision {
                *w.input.lock().unwrap() = Some(WorkerMessage::Config(Box::new(WorkerConfig {
                    device: device.clone(),
                    state: state.clone(),
                    data: data.clone(),
                    preview_screen: selected_preview
                        .as_ref()
                        .filter(|(id, _)| id == &device.id)
                        .map(|(_, s)| s.clone()),
                })));
                w.revision = revision;
            }
        }
        match server.poll() {
            Ok(Some(request)) => {
                let include_preview = matches!(&request, Request::Snapshot { .. });
                let mut response = Response {
                    version: IPC_VERSION,
                    devices: devices.clone(),
                    statuses: workers.values().map(|w| w.status.clone()).collect(),
                    data: data.clone(),
                    ..Default::default()
                };
                let result = (|| -> Result<()> {
                    match request {
                        Request::Status => {}
                        Request::Snapshot { preview } => {
                            if preview != selected_preview {
                                revision += 1;
                            }
                            selected_preview = preview;
                        }
                        Request::SetState(next) => {
                            next.validate()?;
                            if next.settings.auto_start != state.settings.auto_start {
                                crate::platform::autostart(next.settings.auto_start)?;
                            }
                            store.save(&next)?;
                            state = next;
                            revision += 1;
                        }
                        Request::Reconnect(id) => {
                            workers.remove(&id);
                            previews.retain(|(d, _), _| d != &id);
                        }
                        Request::CrashSimulation(id) => {
                            ensure!(
                                simulate
                                    && devices
                                        .iter()
                                        .any(|d| d.id == id && d.adapter == "simulator"),
                                "故障注入仅适用于模拟设备"
                            );
                            workers
                                .get_mut(&id)
                                .ok_or_else(|| anyhow::anyhow!("模拟设备未运行"))?
                                .child
                                .kill()?;
                        }
                        Request::SetKey(key) => {
                            let (tx, rx) = mpsc::channel();
                            service_tx.send(ServiceCommand::Key(Some(key), tx))?;
                            rx.recv_timeout(Duration::from_secs(15))?
                                .map_err(anyhow::Error::msg)?;
                        }
                        Request::ClearKey => {
                            let (tx, rx) = mpsc::channel();
                            service_tx.send(ServiceCommand::Key(None, tx))?;
                            rx.recv_timeout(Duration::from_secs(15))?
                                .map_err(anyhow::Error::msg)?;
                        }
                        Request::Refresh => {
                            service_tx.send(ServiceCommand::Refresh)?;
                        }
                        Request::ReloadServices => {
                            service_tx.send(ServiceCommand::Reload)?;
                        }
                        Request::Quit => running = false,
                    }
                    Ok(())
                })();
                if let Err(e) = result {
                    response.error = Some(e.to_string());
                }
                response.preview = if include_preview {
                    selected_preview
                        .as_ref()
                        .and_then(|k| previews.get(k).cloned())
                } else {
                    None
                };
                let _ = server.respond(&response);
            }
            Ok(None) => {}
            Err(e) => store.log(&format!("IPC 请求失败：{e}")),
        }
        thread::sleep(Duration::from_millis(8));
    }
    let _ = service_tx.send(ServiceCommand::Stop);
    service_alive.store(false, std::sync::atomic::Ordering::Relaxed);
    drop(workers);
    Ok(())
}
enum ServiceCommand {
    Key(
        Option<String>,
        mpsc::Sender<std::result::Result<(), String>>,
    ),
    Refresh,
    Reload,
    Stop,
}
pub fn worker(store: &Storage) -> Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let incoming = Arc::new(Mutex::new(None::<WorkerMessage>));
    let reader = incoming.clone();
    let alive = Arc::new(AtomicBool::new(true));
    let reader_alive = alive.clone();
    thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        while let Ok(message) = ipc::read(&mut stdin) {
            *reader.lock().unwrap() = Some(message);
        }
        reader_alive.store(false, Ordering::Relaxed);
    });
    let render_config = Arc::new(Mutex::new(None::<WorkerConfig>));
    let render_input = render_config.clone();
    let frame_cache = Arc::new(Mutex::new(BTreeMap::<
        ScreenId,
        (u64, Arc<pixelferry_render::Frame>),
    >::new()));
    let rendered = frame_cache.clone();
    let render_error = Arc::new(Mutex::new(None::<String>));
    let errors = render_error.clone();
    let render_alive = alive.clone();
    let media = store.media();
    let tools = default_tools();
    let render_thread = thread::spawn(move || {
        let mut renderers = BTreeMap::<ScreenId, Renderer>::new();
        let mut deadlines = BTreeMap::<ScreenId, Instant>::new();
        let mut sequence = 0;
        while render_alive.load(Ordering::Relaxed) {
            let config = render_input.lock().unwrap().clone();
            let Some(c) = config else {
                thread::sleep(Duration::from_millis(5));
                continue;
            };
            let active: std::collections::BTreeSet<_> = c
                .state
                .bindings
                .iter()
                .filter(|b| b.device_id == c.device.id && b.settings.power)
                .map(|b| b.screen_id.clone())
                .collect();
            renderers.retain(|id, _| active.contains(id));
            rendered.lock().unwrap().retain(|id, _| active.contains(id));
            for screen in &c.device.screens {
                let Some(binding) = c.state.bindings.iter().find(|b| {
                    b.device_id == c.device.id && b.screen_id == screen.id && b.settings.power
                }) else {
                    continue;
                };
                let Some(scene) = c.state.scenes.iter().find(|s| s.id == binding.scene_id) else {
                    continue;
                };
                let start = Instant::now();
                if deadlines.get(&screen.id).is_some_and(|t| start < *t) {
                    continue;
                }
                let renderer = renderers
                    .entry(screen.id.clone())
                    .or_insert_with(|| Renderer::new(&media, &tools));
                match renderer.render(pixelferry_render::RenderRequest {
                    scene,
                    assets: &c.state.assets,
                    data: &c.data,
                    prefs: &c.state.deepseek,
                    size: (screen.width, screen.height),
                    fit: binding.fit,
                    direction: if screen.direction {
                        binding.settings.direction
                    } else {
                        0
                    },
                    quality: binding.settings.quality,
                    format: screen.encoding,
                    now: services::now(),
                }) {
                    Ok(frame) => {
                        sequence += 1;
                        rendered
                            .lock()
                            .unwrap()
                            .insert(screen.id.clone(), (sequence, Arc::new(frame)));
                    }
                    Err(e) => {
                        *errors.lock().unwrap() = Some(e.to_string());
                    }
                }
                if let Some(notice) = renderer.notices.last() {
                    *errors.lock().unwrap() = Some(notice.clone());
                }
                let dynamic = scene
                    .layers
                    .iter()
                    .any(|l| l.visible && l.kind == LayerKind::Video);
                let interval = if dynamic {
                    1000 / binding.settings.render_fps as u64
                } else {
                    1000
                };
                deadlines.insert(screen.id.clone(), start + Duration::from_millis(interval));
            }
            thread::sleep(Duration::from_millis(2));
        }
    });
    let mut config = None::<WorkerConfig>;
    let mut connection = None::<Box<dyn Connection>>;
    let mut controls = BTreeMap::<ScreenId, Settings>::new();
    let mut deadlines = BTreeMap::<ScreenId, Instant>::new();
    let mut status = DeviceStatus::default();
    let mut next_connect = Instant::now();
    let mut last_stats = Instant::now();
    let mut previous_frames = 0;
    let mut bytes = 0u64;
    let mut last_preview = Instant::now();
    let mut output = std::io::stdout().lock();
    while alive.load(Ordering::Relaxed) {
        if let Some(message) = incoming.lock().unwrap().take() {
            match message {
                WorkerMessage::Stop => break,
                WorkerMessage::Config(c) => {
                    let c = *c;
                    status.device_id = c.device.id.clone();
                    *render_config.lock().unwrap() = Some(c.clone());
                    config = Some(c);
                }
            }
        }
        let Some(c) = config.as_ref() else {
            thread::sleep(Duration::from_millis(5));
            continue;
        };
        if connection.is_none() && Instant::now() >= next_connect {
            match devices::adapter(&c.device.adapter)?.connect(&c.device) {
                Ok(conn) => {
                    connection = Some(conn);
                    controls.clear();
                    status.state = "connected".into();
                    status.message = "已连接".into();
                }
                Err(e) => {
                    status.state = "disconnected".into();
                    status.message = e.to_string();
                    next_connect = Instant::now() + Duration::from_secs(2);
                }
            }
        }
        let mut error = None;
        for screen in &c.device.screens {
            let Some(binding) = c
                .state
                .bindings
                .iter()
                .find(|b| b.device_id == c.device.id && b.screen_id == screen.id)
            else {
                continue;
            };
            if let Some(conn) = connection.as_mut() {
                if controls.get(&screen.id) != Some(&binding.settings) {
                    if let Err(e) = conn.controls(&screen.id, &binding.settings) {
                        error = Some(e.to_string());
                        break;
                    }
                    controls.insert(screen.id.clone(), binding.settings.clone());
                }
            }
            if !binding.settings.power {
                continue;
            }
            let start = Instant::now();
            if deadlines.get(&screen.id).is_some_and(|t| start < *t) {
                continue;
            }
            let frame = frame_cache.lock().unwrap().get(&screen.id).cloned();
            if let (Some(conn), Some((_, frame))) = (connection.as_mut(), frame) {
                match conn.send(&screen.id, &frame.output()) {
                    Ok(n) => {
                        bytes += n as u64;
                        status.frames += 1;
                        *status.screen_frames.entry(screen.id.clone()).or_default() += 1;
                    }
                    Err(e) => {
                        error = Some(e.to_string());
                        break;
                    }
                }
            }
            deadlines.insert(
                screen.id.clone(),
                start + Duration::from_micros(1_000_000 / screen.repeat_fps.max(1) as u64),
            );
        }
        if let Some(message) = error {
            status.errors += 1;
            status.state = "error".into();
            status.message = message;
            connection = None;
            next_connect = Instant::now() + Duration::from_secs(2);
        }
        if let Some(message) = render_error.lock().unwrap().take() {
            status.errors += 1;
            status.message = format!("画面生成：{message}");
        }
        if last_stats.elapsed() > Duration::from_millis(500) {
            let seconds = last_stats.elapsed().as_secs_f64();
            status.fps = (status.frames - previous_frames) as f64 / seconds;
            status.bytes_per_second = bytes as f64 / seconds;
            previous_frames = status.frames;
            bytes = 0;
            last_stats = Instant::now();
            let preview = if last_preview.elapsed() >= Duration::from_millis(200) {
                c.preview_screen.as_ref().and_then(|id| {
                    frame_cache
                        .lock()
                        .unwrap()
                        .get(id)
                        .cloned()
                        .map(|(sequence, frame)| (id.clone(), small_preview(&frame, sequence)))
                })
            } else {
                None
            };
            if preview.is_some() {
                last_preview = Instant::now();
            }
            ipc::write(
                &mut output,
                &WorkerEvent {
                    status: status.clone(),
                    preview,
                },
            )?;
        }
        thread::sleep(Duration::from_millis(1));
    }
    alive.store(false, Ordering::Relaxed);
    let _ = render_thread.join();
    Ok(())
}
fn small_preview(frame: &pixelferry_render::Frame, sequence: u64) -> Preview {
    let scale = (640. / frame.width.max(frame.height) as f64).min(1.);
    let width = (frame.width as f64 * scale).round().max(1.) as u32;
    let height = (frame.height as f64 * scale).round().max(1.) as u32;
    let mut rgba = vec![0; width as usize * height as usize * 4];
    for y in 0..height {
        for x in 0..width {
            let sx = x as u64 * frame.width as u64 / width as u64;
            let sy = y as u64 * frame.height as u64 / height as u64;
            let from = (sy * frame.width as u64 + sx) as usize * 4;
            let to = (y as usize * width as usize + x as usize) * 4;
            rgba[to..to + 4].copy_from_slice(&frame.rgba[from..from + 4]);
        }
    }
    Preview {
        width,
        height,
        rgba: base64::engine::general_purpose::STANDARD.encode(rgba),
        sequence,
    }
}
pub fn ensure_runtime(store: &Storage, simulate: bool) -> Result<()> {
    match ipc::request(&store.root, Request::Snapshot { preview: None }) {
        Ok(_) => return Ok(()),
        Err(e) if e.to_string().contains("版本不匹配") => return Err(e),
        Err(_) => {}
    }
    let mut c = Command::new(std::env::current_exe()?);
    quiet(&mut c);
    c.arg("--runtime")
        .arg(format!("--data-dir={}", store.root.display()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if simulate {
        c.arg("--simulate");
    }
    let mut child = c.spawn()?;
    for _ in 0..100 {
        if ipc::request(&store.root, Request::Snapshot { preview: None }).is_ok() {
            return Ok(());
        }
        if child.try_wait()?.is_some() {
            anyhow::bail!("PixelFerry 后台启动失败")
        }
        thread::sleep(Duration::from_millis(50));
    }
    anyhow::bail!("PixelFerry 后台启动超时")
}
pub fn self_test(store: &Storage) -> Result<()> {
    use devices::Adapter;
    ensure!(
        !store.root.join("state.json").exists(),
        "自动验收需要空数据目录，不会覆盖已有配置"
    );
    let mut state = AppState::default();
    let simulated = devices::Simulator.discover()?;
    for (index, device) in simulated.iter().enumerate() {
        for screen in &device.screens {
            let mut scene = Scene::blank(
                &format!("{} {}", device.name, screen.name),
                screen.width,
                screen.height,
            );
            scene.background = ["#ff0000", "#00ff00", "#0000ff"][index].into();
            state.bindings.push(OutputBinding {
                device_id: device.id.clone(),
                screen_id: screen.id.clone(),
                scene_id: scene.id.clone(),
                fit: Fit::Contain,
                settings: Settings::default(),
            });
            state.scenes.push(scene);
        }
    }
    store.save(&state)?;
    ensure_runtime(store, true)?;
    struct Stop(std::path::PathBuf);
    impl Drop for Stop {
        fn drop(&mut self) {
            let _ = ipc::request(&self.0, Request::Quit);
        }
    }
    let _stop = Stop(store.root.clone());
    ipc::request(&store.root, Request::SetState(state.clone()))?;
    let mut last = Response::default();
    for (index, device) in simulated.iter().enumerate() {
        for screen in &device.screens {
            let start = Instant::now();
            let mut valid = false;
            while start.elapsed() < Duration::from_secs(15) {
                last = ipc::request(
                    &store.root,
                    Request::Snapshot {
                        preview: Some((device.id.clone(), screen.id.clone())),
                    },
                )?;
                if let Some(preview) = &last.preview {
                    let bytes = base64::engine::general_purpose::STANDARD.decode(&preview.rgba)?;
                    let expected = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]][index];
                    if preview.width == screen.width
                        && preview.height == screen.height
                        && bytes.get(..4) == Some(&expected)
                    {
                        valid = true;
                        break;
                    }
                }
                thread::sleep(Duration::from_millis(100));
            }
            ensure!(valid, "{} 的输出画面不匹配", screen.name);
        }
    }
    ensure!(
        last.statuses.len() == 3
            && last
                .statuses
                .iter()
                .map(|s| s.screen_frames.len())
                .sum::<usize>()
                == 5,
        "五屏并行运行失败"
    );
    // An asymmetric marker verifies settings propagation through IPC and the
    // actual render worker, including changes while the device stays connected.
    let scene_id = state.bindings[0].scene_id.clone();
    let scene = state.scenes.iter_mut().find(|s| s.id == scene_id).unwrap();
    scene.background = "#000000".into();
    scene.layers.push(Layer {
        x: 0.,
        y: 0.,
        width: 120.,
        height: 120.,
        color: "#ff0000".into(),
        corner_radius: 0.,
        ..Layer::new(LayerKind::Rect)
    });
    let device = &simulated[0];
    let screen = &device.screens[0];
    for direction in 0..=3 {
        state.bindings[0].settings.direction = direction;
        ipc::request(&store.root, Request::SetState(state.clone()))?;
        let start = Instant::now();
        let mut valid = false;
        while start.elapsed() < Duration::from_secs(15) {
            last = ipc::request(
                &store.root,
                Request::Snapshot {
                    preview: Some((device.id.clone(), screen.id.clone())),
                },
            )?;
            if let Some(preview) = &last.preview {
                let bytes = base64::engine::general_purpose::STANDARD.decode(&preview.rgba)?;
                if (preview.width, preview.height) == (480, 480) {
                    valid = [(60, 60), (420, 60), (420, 420), (60, 420)]
                        .iter()
                        .enumerate()
                        .all(|(corner, &(x, y))| {
                            let i = (y * 480 + x) * 4;
                            let expected = if corner == direction as usize {
                                [255, 0, 0, 255]
                            } else {
                                [0, 0, 0, 255]
                            };
                            bytes.get(i..i + 4) == Some(&expected)
                        });
                }
            }
            if valid {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        ensure!(
            valid,
            "安装方向 {}° 的后台输出画面不匹配",
            u16::from(direction) * 90
        );
    }
    // Compare a mixed-text frame against a local render, through the real IPC
    // worker path, without requiring account credentials or physical sensors.
    let scene = state.scenes.iter_mut().find(|s| s.id == scene_id).unwrap();
    scene.layers.push(Layer {
        x: 120.,
        y: 180.,
        width: 300.,
        height: 180.,
        text: "余额：[DeepSeek · 余额]".into(),
        vertical_align: "bottom".into(),
        rich_text: Some(RichText {
            version: 1,
            runs: vec![
                RichRun::Text {
                    text: "余额：".into(),
                    style: RichStyle {
                        color: Some("#ffcc00".into()),
                        ..Default::default()
                    },
                },
                RichRun::Deepseek {
                    metric: "balance".into(),
                    decimals: 2,
                    show_currency: true,
                    affix_scale: None,
                    style: RichStyle::default(),
                },
                RichRun::Text {
                    text: "\nGPU：".into(),
                    style: RichStyle::default(),
                },
                RichRun::Sensor {
                    sensor_id: "__selftest_missing_sensor__".into(),
                    label: "测试传感器".into(),
                    decimals: 1,
                    show_unit: true,
                    affix_scale: None,
                    style: RichStyle {
                        font_size: Some(40.),
                        ..Default::default()
                    },
                },
            ],
        }),
        ..Layer::new(LayerKind::Text)
    });
    let expected = Renderer::new(&store.media(), &default_tools())
        .render(pixelferry_render::RenderRequest {
            scene,
            assets: &state.assets,
            data: &DataSnapshot::default(),
            prefs: &state.deepseek,
            size: (480, 480),
            fit: Fit::Contain,
            direction: 0,
            quality: 70,
            format: FrameFormat::Jpeg,
            now: services::now(),
        })?
        .rgba;
    state.bindings[0].settings.direction = 0;
    ipc::request(&store.root, Request::SetState(state.clone()))?;
    let start = Instant::now();
    let mut mixed_valid = false;
    while start.elapsed() < Duration::from_secs(15) {
        last = ipc::request(
            &store.root,
            Request::Snapshot {
                preview: Some((device.id.clone(), screen.id.clone())),
            },
        )?;
        if let Some(preview) = &last.preview {
            mixed_valid =
                base64::engine::general_purpose::STANDARD.decode(&preview.rgba)? == expected;
        }
        if mixed_valid {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    ensure!(mixed_valid, "混排文字的后台输出画面不匹配");
    let before: BTreeMap<_, _> = last
        .statuses
        .iter()
        .map(|s| (s.device_id.clone(), s.frames))
        .collect();
    let failed = simulated[1].id.clone();
    ipc::request(&store.root, Request::CrashSimulation(failed.clone()))?;
    let start = Instant::now();
    let mut recovered = false;
    while start.elapsed() < Duration::from_secs(15) {
        last = ipc::request(
            &store.root,
            Request::Snapshot {
                preview: Some((simulated[0].id.clone(), simulated[0].screens[0].id.clone())),
            },
        )?;
        recovered = last
            .statuses
            .iter()
            .any(|s| s.device_id == failed && s.state == "connected" && s.frames > 0)
            && last
                .statuses
                .iter()
                .filter(|s| s.device_id != failed)
                .all(|s| s.frames > *before.get(&s.device_id).unwrap_or(&0));
        if recovered {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    ensure!(recovered, "设备进程异常退出后的隔离与恢复失败");
    let report = serde_json::json!({"passed":true,"devices":3,"screens":5,"independentPixels":true,"installationDirections":true,"mixedText":true,"workerCrashRecovery":true,"statuses":last.statuses});
    std::fs::write(
        store.root.join("selftest.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

/// Samples the already-running application; it never opens or binds hardware.
pub fn monitor(store: &Storage, seconds: u64) -> Result<()> {
    use std::io::Write;
    ensure!((1..=86400).contains(&seconds), "采样时长必须为 1–86400 秒");
    let first = ipc::request(&store.root, Request::Status)?;
    ensure!(
        first.statuses.iter().any(|s| s.state == "connected"),
        "请先在 PixelFerry 绑定并连接屏幕"
    );
    let directory = store.root.join("artifacts");
    std::fs::create_dir_all(&directory)?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let log_path = directory.join(format!("soak-{stamp}.jsonl"));
    let summary_path = directory.join(format!("soak-{stamp}-summary.json"));
    let mut file = std::fs::File::create(&log_path)?;
    let start = Instant::now();
    let mut samples = 0u64;
    let mut unhealthy = 0u64;
    let mut previous: BTreeMap<_, _> = first
        .statuses
        .iter()
        .map(|s| (s.device_id.clone(), s.frames))
        .collect();
    let mut restarts = 0u64;
    loop {
        let response = ipc::request(&store.root, Request::Status)?;
        let elapsed = start.elapsed().as_secs_f64();
        if response.statuses.is_empty() || response.statuses.iter().any(|s| s.state != "connected")
        {
            unhealthy += 1;
        }
        for status in &response.statuses {
            if previous
                .get(&status.device_id)
                .is_some_and(|n| status.frames < *n)
            {
                restarts += 1;
            }
            previous.insert(status.device_id.clone(), status.frames);
        }
        writeln!(
            file,
            "{}",
            serde_json::to_string(
                &serde_json::json!({"elapsedSeconds":elapsed,"statuses":response.statuses})
            )?
        )?;
        samples += 1;
        if elapsed >= seconds as f64 {
            file.sync_all()?;
            let summary = serde_json::json!({"requestedSeconds":seconds,"elapsedSeconds":elapsed,"completed":true,"samples":samples,"unhealthySamples":unhealthy,"workerCounterResets":restarts,"devices":first.devices,"log":log_path,"screenVisuallyVerified":false,"note":"此记录仅证明应用状态与写入计数，实屏画面需人工验收"});
            std::fs::write(&summary_path, serde_json::to_vec_pretty(&summary)?)?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
            break;
        }
        thread::sleep(Duration::from_secs(1));
    }
    Ok(())
}
