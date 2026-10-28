use crate::{
    ipc::{self, Preview, Request, Response},
    runtime,
};
use anyhow::Result;
use base64::Engine;
use pixelferry_core::*;
use pixelferry_render::{DataSnapshot, RenderRequest, Renderer};
use pixelferry_storage::{self as storage, Storage};
use std::sync::{mpsc, Arc, Mutex};
use tauri::{Emitter, Manager};
type ApiResult<T> = std::result::Result<T, String>;
#[derive(Clone)]
struct Backend {
    store: Storage,
    io: Arc<Mutex<()>>,
    data: Arc<Mutex<DataSnapshot>>,
    render: mpsc::SyncSender<RenderJob>,
}
struct RenderJob {
    state: AppState,
    data: DataSnapshot,
    reply: mpsc::SyncSender<ApiResult<Preview>>,
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
impl Backend {
    fn request(&self, request: Request) -> ApiResult<Response> {
        let _guard = self.io.lock().map_err(err)?;
        let response = ipc::request(&self.store.root, request).map_err(err)?;
        *self.data.lock().map_err(err)? = response.data.clone();
        Ok(response)
    }
}
#[tauri::command]
async fn load_workspace(state: tauri::State<'_, Backend>) -> ApiResult<AppState> {
    let b = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = b.io.lock().map_err(err)?;
        b.store.load().map_err(err)
    })
    .await
    .map_err(err)?
}
#[tauri::command]
async fn save_workspace(state: tauri::State<'_, Backend>, doc: AppState) -> ApiResult<()> {
    let b = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        doc.validate().map_err(err)?;
        b.request(Request::SetState(doc))?;
        Ok(())
    })
    .await
    .map_err(err)?
}
#[tauri::command]
async fn snapshot(
    state: tauri::State<'_, Backend>,
    device: Option<String>,
    screen: Option<String>,
) -> ApiResult<Response> {
    let b = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        b.request(Request::Snapshot {
            preview: device.zip(screen).map(|(d, s)| (DeviceId(d), ScreenId(s))),
        })
    })
    .await
    .map_err(err)?
}
#[tauri::command]
async fn render_editor(state: tauri::State<'_, Backend>, doc: AppState) -> ApiResult<Preview> {
    let b = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        doc.validate().map_err(err)?;
        let (tx, rx) = mpsc::sync_channel(1);
        b.render
            .send(RenderJob {
                state: doc,
                data: b.data.lock().map_err(err)?.clone(),
                reply: tx,
            })
            .map_err(err)?;
        rx.recv_timeout(std::time::Duration::from_secs(15))
            .map_err(err)?
    })
    .await
    .map_err(err)?
}
#[tauri::command]
async fn import_media(state: tauri::State<'_, Backend>) -> ApiResult<Option<Asset>> {
    let b = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let file = rfd::FileDialog::new()
            .set_title("带些像素上船")
            .add_filter(
                "媒体",
                &[
                    "png", "jpg", "jpeg", "webp", "bmp", "avif", "gif", "mp4", "mov", "mkv",
                    "webm", "avi",
                ],
            )
            .pick_file();
        file.map(|p| {
            storage::media::import(&p, &b.store.media(), &storage::default_tools()).map_err(err)
        })
        .transpose()
    })
    .await
    .map_err(err)?
}
#[tauri::command]
async fn export_scenes(
    state: tauri::State<'_, Backend>,
    doc: AppState,
    all: bool,
) -> ApiResult<bool> {
    let b = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(file) = rfd::FileDialog::new()
            .add_filter("PixelFerry 场景包", &["pixelferry"])
            .set_file_name("scenes.pixelferry")
            .save_file()
        else {
            return Ok(false);
        };
        let _guard = b.io.lock().map_err(err)?;
        storage::packages::export(&b.store, &doc, &file, all).map_err(err)?;
        Ok(true)
    })
    .await
    .map_err(err)?
}
#[tauri::command]
async fn import_scenes(
    state: tauri::State<'_, Backend>,
    doc: AppState,
) -> ApiResult<Option<AppState>> {
    let b = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(file) = rfd::FileDialog::new()
            .add_filter("场景包", &["pixelferry", "lg600", "zip"])
            .pick_file()
        else {
            return Ok(None);
        };
        let _guard = b.io.lock().map_err(err)?;
        let next = storage::packages::import(&b.store, &doc, &file).map_err(err)?;
        ipc::request(&b.store.root, Request::SetState(next.clone())).map_err(err)?;
        Ok(Some(next))
    })
    .await
    .map_err(err)?
}
#[derive(serde::Serialize)]
struct Migration {
    doc: AppState,
    notices: Vec<String>,
}
#[tauri::command]
async fn migrate_legacy(
    state: tauri::State<'_, Backend>,
    doc: AppState,
) -> ApiResult<Option<Migration>> {
    let b = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(source) = rfd::FileDialog::new()
            .set_title("选择包含旧 state.json 的目录")
            .pick_folder()
        else {
            return Ok(None);
        };
        let _guard = b.io.lock().map_err(err)?;
        let (next, notices) = storage::migrate_legacy(&b.store, &doc, &source).map_err(err)?;
        ipc::request(&b.store.root, Request::SetState(next.clone())).map_err(err)?;
        ipc::request(&b.store.root, Request::ReloadServices).map_err(err)?;
        Ok(Some(Migration { doc: next, notices }))
    })
    .await
    .map_err(err)?
}
#[tauri::command]
async fn reconnect(state: tauri::State<'_, Backend>, device: String) -> ApiResult<()> {
    let b = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        b.request(Request::Reconnect(DeviceId(device))).map(|_| ())
    })
    .await
    .map_err(err)?
}
#[tauri::command]
async fn set_key(state: tauri::State<'_, Backend>, key: Option<String>) -> ApiResult<()> {
    let b = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        b.request(key.map(Request::SetKey).unwrap_or(Request::ClearKey))
            .map(|_| ())
    })
    .await
    .map_err(err)?
}
#[tauri::command]
async fn refresh_account(state: tauri::State<'_, Backend>) -> ApiResult<()> {
    let b = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || b.request(Request::Refresh).map(|_| ()))
        .await
        .map_err(err)?
}
#[tauri::command]
fn hide_window(window: tauri::WebviewWindow) -> ApiResult<()> {
    window.hide().map_err(err)
}
#[tauri::command]
async fn quit_app(app: tauri::AppHandle, state: tauri::State<'_, Backend>) -> ApiResult<()> {
    let b = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || b.request(Request::Quit).map(|_| ()))
        .await
        .map_err(err)??;
    app.exit(0);
    Ok(())
}
#[tauri::command]
fn app_info() -> serde_json::Value {
    serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"simulate":std::env::args().any(|a|a=="--simulate"),"name":"PixelFerry"})
}
pub fn run(store: Storage, simulate: bool) -> Result<()> {
    let Some(_instance) = crate::instance::acquire(&store.root)? else {
        return Ok(());
    };
    if !store.root.join("state.json").exists() {
        store.save(&store.load()?)?;
    }
    runtime::ensure_runtime(&store, simulate)?;
    let (tx, rx) = mpsc::sync_channel::<RenderJob>(1);
    let media = store.media();
    let tools = storage::default_tools();
    std::thread::spawn(move || {
        let mut renderer = Renderer::new(&media, &tools);
        while let Ok(job) = rx.recv() {
            let scene = job.state.active_scene();
            let result = renderer
                .render(RenderRequest {
                    scene,
                    assets: &job.state.assets,
                    data: &job.data,
                    prefs: &job.state.deepseek,
                    size: (scene.width, scene.height),
                    fit: Fit::Contain,
                    direction: 0,
                    quality: 70,
                    format: FrameFormat::Rgba8,
                    now: pixelferry_services::now(),
                })
                .map(|f| Preview {
                    width: f.width,
                    height: f.height,
                    rgba: base64::engine::general_purpose::STANDARD.encode(f.rgba),
                    sequence: pixelferry_services::now() as u64,
                })
                .map_err(err);
            let _ = job.reply.send(result);
        }
    });
    let backend = Backend {
        store,
        io: Arc::new(Mutex::new(())),
        data: Arc::new(Mutex::new(DataSnapshot::default())),
        render: tx,
    };
    tauri::Builder::default()
        .manage(backend)
        .invoke_handler(tauri::generate_handler![
            load_workspace,
            save_workspace,
            snapshot,
            render_editor,
            import_media,
            export_scenes,
            import_scenes,
            migrate_legacy,
            reconnect,
            set_key,
            refresh_account,
            hide_window,
            quit_app,
            app_info
        ])
        .setup(|app| {
            use tauri::{
                menu::{Menu, MenuItem},
                tray::TrayIconBuilder,
            };
            let open = MenuItem::with_id(app, "open", "打开 PixelFerry", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "完全退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;
            let mut tray = TrayIconBuilder::new()
                .menu(&menu)
                .tooltip("PixelFerry · 像素渡轮")
                .on_menu_event(|app, event| {
                    if event.id.as_ref() == "open" {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    if event.id.as_ref() == "quit" {
                        let _ = app.emit("pf-quit", ());
                        let app = app.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_secs(5));
                            let b = app.state::<Backend>();
                            let _ = b.request(Request::Quit);
                            app.exit(0);
                        });
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            if std::env::args().any(|a| a == "--minimized") {
                if let Some(w) = app.get_webview_window("main") {
                    w.hide()?;
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                let _ = window.emit("pf-hide", ());
            }
        })
        .run(tauri::generate_context!())
        .map_err(|e| anyhow::anyhow!(e.to_string()))
}
