#![cfg_attr(windows, windows_subsystem = "windows")]
mod instance;
mod ipc;
mod platform;
mod runtime;
mod ui;
use anyhow::Result;
use pixelferry_storage::{default_root, Storage};
use std::path::PathBuf;
fn main() {
    if let Err(e) = run() {
        eprintln!("PixelFerry: {e:#}");
        #[cfg(windows)]
        if !std::env::args().any(|a| {
            [
                "--runtime",
                "--worker",
                "--self-test",
                "--diagnose",
                "--status",
                "--quit",
            ]
            .contains(&a.as_str())
                || a.starts_with("--soak-seconds=")
        }) {
            let message: Vec<u16> = format!("PixelFerry 启动失败：{e:#}")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let title: Vec<u16> = "PixelFerry".encode_utf16().chain(Some(0)).collect();
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                    std::ptr::null_mut(),
                    message.as_ptr(),
                    title.as_ptr(),
                    0x10,
                );
            }
        }
        std::process::exit(1)
    }
}
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let root = args
        .iter()
        .find_map(|s| s.strip_prefix("--data-dir="))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if args.iter().any(|a| a == "--self-test") {
                std::env::temp_dir().join(format!("PixelFerry-selftest-{}", uuid::Uuid::new_v4()))
            } else {
                default_root()
            }
        });
    let simulate = args.iter().any(|s| s == "--simulate");
    let store = Storage::new(root)?;
    if args.iter().any(|s| s == "--status") {
        let response = ipc::request(&store.root, ipc::Request::Status)?;
        println!("{}", serde_json::to_string_pretty(&response.statuses)?);
        return Ok(());
    }
    if let Some(seconds) = args.iter().find_map(|a| a.strip_prefix("--soak-seconds=")) {
        return runtime::monitor(&store, seconds.parse()?);
    }
    if args.iter().any(|s| s == "--quit") {
        ipc::request(&store.root, ipc::Request::Quit)?;
        return Ok(());
    }
    if args.iter().any(|s| s == "--worker") {
        return runtime::worker(&store);
    }
    if args.iter().any(|s| s == "--runtime") {
        return runtime::serve(store, simulate);
    }
    if args.iter().any(|s| s == "--diagnose") {
        println!(
            "{}",
            serde_json::to_string_pretty(&pixelferry_devices::discover(simulate)?)?
        );
        return Ok(());
    }
    if args.iter().any(|s| s == "--self-test") {
        return runtime::self_test(&store);
    }
    ui::run(store, simulate)
}
