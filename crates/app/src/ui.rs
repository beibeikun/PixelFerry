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
    scene_id: Option<String>,
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
                scene_id: None,
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
async fn render_thumbnail(
    state: tauri::State<'_, Backend>,
    doc: AppState,
    scene_id: String,
) -> ApiResult<Preview> {
    let b = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        doc.validate().map_err(err)?;
        if !doc.scenes.iter().any(|s| s.id.0 == scene_id) {
            return Err("场景不存在".into());
        }
        let (tx, rx) = mpsc::sync_channel(1);
        b.render
            .send(RenderJob {
                state: doc,
                data: b.data.lock().map_err(err)?.clone(),
                scene_id: Some(scene_id),
                reply: tx,
            })
            .map_err(err)?;
        rx.recv_timeout(std::time::Duration::from_secs(15))
            .map_err(err)?
    })
    .await
    .map_err(err)?
}
fn thumbnail_size(width: u32, height: u32) -> (u32, u32) {
    let scale = (320. / width.max(height) as f64).min(1.);
    (
        (width as f64 * scale).round().max(1.) as u32,
        (height as f64 * scale).round().max(1.) as u32,
    )
}
#[cfg(test)]
mod thumbnail_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn thumbnail_dimensions_preserve_landscape_portrait_and_small_scenes() {
        assert_eq!(thumbnail_size(1920, 1080), (320, 180));
        assert_eq!(thumbnail_size(1080, 1920), (180, 320));
        assert_eq!(thumbnail_size(480, 480), (320, 320));
        assert_eq!(thumbnail_size(10, 20), (10, 20));
        assert_eq!(thumbnail_size(8192, 1), (320, 1));
    }

    #[test]
    fn renders_requested_scene_without_changing_document_or_bindings() {
        let mut state = AppState::default();
        state.scenes = vec![
            Scene::blank("red", 640, 320),
            Scene::blank("blue", 320, 640),
        ];
        state.scenes[0].background = "#ff0000".into();
        state.scenes[1].background = "#0000ff".into();
        state.active_scene_id = state.scenes[0].id.clone();
        let saved = serde_json::to_value(&state).unwrap();
        let mut renderer = Renderer::new(Path::new("."), Path::new("."));
        let data = DataSnapshot::default();
        for (index, color) in [(0, [255, 0, 0, 255]), (1, [0, 0, 255, 255])] {
            let frame = render_scene(
                &state,
                &data,
                Some(&state.scenes[index].id.0),
                &mut renderer,
            )
            .unwrap();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&frame.rgba)
                .unwrap();
            assert_eq!(
                (frame.width, frame.height),
                if index == 0 { (320, 160) } else { (160, 320) }
            );
            assert!(bytes.chunks_exact(4).all(|p| p == color));
        }
        assert!(render_scene(&state, &data, Some("missing"), &mut renderer).is_err());
        assert_eq!(serde_json::to_value(&state).unwrap(), saved);
        let editor = render_scene(&state, &data, None, &mut renderer).unwrap();
        assert_eq!((editor.width, editor.height), (640, 320));
    }
}
fn render_scene(
    state: &AppState,
    data: &DataSnapshot,
    scene_id: Option<&str>,
    renderer: &mut Renderer,
) -> ApiResult<Preview> {
    let scene = match scene_id {
        Some(id) => state
            .scenes
            .iter()
            .find(|s| s.id.0 == id)
            .ok_or("场景不存在")?,
        None => state.active_scene(),
    };
    let request = RenderRequest {
        scene,
        assets: &state.assets,
        data,
        prefs: &state.deepseek,
        size: if scene_id.is_some() {
            thumbnail_size(scene.width, scene.height)
        } else {
            (scene.width, scene.height)
        },
        fit: Fit::Contain,
        direction: 0,
        quality: 70,
        format: FrameFormat::Rgba8,
        now: pixelferry_services::now(),
    };
    let frame = if scene_id.is_some() {
        renderer.render_thumbnail(request)
    } else {
        renderer.render(request)
    }
    .map_err(err)?;
    Ok(Preview {
        width: frame.width,
        height: frame.height,
        rgba: base64::engine::general_purpose::STANDARD.encode(frame.rgba),
        sequence: pixelferry_services::now() as u64,
    })
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
        let mut thumbnails = Renderer::new(&media, &tools);
        while let Ok(job) = rx.recv() {
            let target = if job.scene_id.is_some() {
                &mut thumbnails
            } else {
                &mut renderer
            };
            let result = render_scene(&job.state, &job.data, job.scene_id.as_deref(), target);
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
            render_thumbnail,
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
