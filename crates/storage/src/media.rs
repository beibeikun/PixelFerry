use anyhow::{ensure, Context, Result};
use pixelferry_core::{new_id, Asset};
use std::{
    fs,
    io::Read,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
pub fn run(mut command: Command, timeout: Duration) -> Result<Vec<u8>> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("无法启动媒体工具")?;
    let mut stdout = child.stdout.take().unwrap();
    let reader = thread::spawn(move || {
        let mut b = vec![];
        stdout
            .by_ref()
            .take(1024 * 1024 + 1)
            .read_to_end(&mut b)
            .map(|_| b)
    });
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("媒体处理超时")
        }
        thread::sleep(Duration::from_millis(20));
    };
    let bytes = reader
        .join()
        .map_err(|_| anyhow::anyhow!("媒体输出读取失败"))??;
    ensure!(
        status.success() && bytes.len() <= 1024 * 1024,
        "媒体处理失败或输出过大"
    );
    Ok(bytes)
}
fn probe(file: &Path, tools: &Path) -> Result<serde_json::Value> {
    let mut c = Command::new(tools.join("ffprobe.exe"));
    c.args([
        "-v",
        "error",
        "-show_streams",
        "-show_format",
        "-of",
        "json",
    ])
    .arg(file);
    Ok(serde_json::from_slice(&run(c, Duration::from_secs(30))?)?)
}
pub fn import(source: &Path, media: &Path, tools: &Path) -> Result<Asset> {
    let info = fs::metadata(source)?;
    ensure!(
        info.is_file() && info.len() <= 1024 * 1024 * 1024,
        "素材必须不超过 1 GB"
    );
    let ext = source
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let image = ["png", "jpg", "jpeg", "webp", "bmp", "avif"].contains(&ext.as_str());
    ensure!(
        image || ["gif", "mp4", "mov", "mkv", "webm", "avi"].contains(&ext.as_str()),
        "不支持的素材格式"
    );
    let meta = probe(source, tools)?;
    let stream = meta["streams"]
        .as_array()
        .and_then(|a| a.iter().find(|s| s["codec_type"] == "video"))
        .context("素材没有视频画面")?;
    let pixels = stream["width"].as_u64().unwrap_or(0) * stream["height"].as_u64().unwrap_or(0);
    ensure!(pixels > 0 && pixels <= 40_000_000, "素材尺寸过大或无效");
    let duration = meta["format"]["duration"]
        .as_str()
        .or(stream["duration"].as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.);
    ensure!(
        duration.is_finite() && (0. ..=600.).contains(&duration),
        "动态素材最长支持 10 分钟"
    );
    if ext == "webp" {
        ensure!(
            stream["nb_frames"]
                .as_str()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(1)
                <= 1,
            "动画 WebP 请先转为 GIF 或 MP4"
        );
    }
    let id = new_id();
    let file = format!("{id}.{}", if image { "png" } else { "mp4" });
    let output = media.join(&file);
    let result = (|| -> Result<Asset> {
        let mut c = Command::new(tools.join("ffmpeg.exe"));
        c.args(["-nostdin", "-v", "error", "-y", "-threads", "2", "-i"])
            .arg(source)
            .args(["-map", "0:v:0"]);
        if image {
            c.args([
                "-frames:v",
                "1",
                "-vf",
                "scale='min(1920,iw)':'min(1920,ih)':force_original_aspect_ratio=decrease",
                "-threads",
                "1",
            ]);
        } else {
            c.args(["-vf","scale=960:960:force_original_aspect_ratio=decrease:force_divisible_by=2,setsar=1,fps=30","-c:v","libx264","-preset","fast","-crf","20","-pix_fmt","yuv420p","-threads","2","-an","-sn","-dn","-map_metadata","-1","-movflags","+faststart"]);
        }
        c.arg(&output);
        run(c, Duration::from_secs(600))?;
        let out = probe(&output, tools)?;
        let s = &out["streams"][0];
        Ok(Asset {
            id,
            name: source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            file,
            kind: if image { "image" } else { "video" }.into(),
            width: s["width"].as_u64().context("输出宽度无效")? as u32,
            height: s["height"].as_u64().context("输出高度无效")? as u32,
            duration: (!image).then_some(duration),
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(output);
    }
    result
}
