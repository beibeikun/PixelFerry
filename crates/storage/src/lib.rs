pub mod media;
pub mod packages;
use anyhow::{ensure, Context, Result};
use pixelferry_core::*;
pub use pixelferry_services::deepseek::atomic;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
#[derive(Clone)]
pub struct Storage {
    pub root: PathBuf,
}
impl Storage {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let mut s = Self { root: root.into() };
        fs::create_dir_all(s.media())?;
        s.root = s.root.canonicalize()?;
        Ok(s)
    }
    pub fn media(&self) -> PathBuf {
        self.root.join("media")
    }
    pub fn load(&self) -> Result<AppState> {
        let file = self.root.join("state.json");
        if !file.exists() {
            return Ok(AppState::default());
        }
        match self.read_state(&file) {
            Ok(s) => Ok(s),
            Err(e) => {
                fs::copy(
                    &file,
                    self.root.join(format!(
                        "state.invalid-{}.json",
                        chrono::Utc::now().timestamp_millis()
                    )),
                )?;
                self.read_state(&self.root.join("state.json.bak"))
                    .with_context(|| format!("配置及备份无法读取，原件已保留：{e}"))
            }
        }
    }
    fn read_state(&self, path: &Path) -> Result<AppState> {
        let s: AppState = serde_json::from_slice(&read_limited(path, 4 * 1024 * 1024)?)?;
        s.validate()?;
        Ok(s)
    }
    pub fn save(&self, state: &AppState) -> Result<()> {
        state.validate()?;
        let path = self.root.join("state.json");
        if path.exists() {
            fs::copy(&path, self.root.join("state.json.bak"))?;
        }
        atomic(&path, &serde_json::to_vec_pretty(state)?)
    }
    pub fn log(&self, message: &str) {
        use std::io::Write;
        let _ = (|| -> Result<()> {
            let dir = self.root.join("logs");
            fs::create_dir_all(&dir)?;
            let file = dir.join("pixelferry.log");
            if fs::metadata(&file)
                .map(|m| m.len() > 2 * 1024 * 1024)
                .unwrap_or(false)
            {
                let _ = fs::rename(&file, dir.join("pixelferry.log.1"));
            }
            let mut f = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(file)?;
            writeln!(f, "{} {}", chrono::Utc::now().to_rfc3339(), message)?;
            Ok(())
        })();
    }
}
pub fn read_limited(path: &Path, max: u64) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= max,
        "文件类型或大小无效"
    );
    let mut bytes = vec![];
    fs::File::open(path)?
        .take(max + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= max, "文件超限");
    Ok(bytes)
}
pub fn default_root() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("PixelFerry")
}
pub fn default_tools() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let adjacent = exe.parent().unwrap_or(Path::new(".")).join("tools");
    if adjacent.join("ffmpeg.exe").exists() {
        return adjacent;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/tools")
}
pub fn migrate_legacy(
    store: &Storage,
    current: &AppState,
    source: &Path,
) -> Result<(AppState, Vec<String>)> {
    let source = source.canonicalize()?;
    let origin = source.to_string_lossy().to_string();
    ensure!(
        source != store.root.canonicalize()?,
        "不能从当前数据目录迁移"
    );
    ensure!(
        current.migrated_from.as_deref() != Some(origin.as_str()),
        "该目录已经迁移，请使用场景包导入追加内容"
    );
    let mut value: serde_json::Value =
        serde_json::from_slice(&read_limited(&source.join("state.json"), 4 * 1024 * 1024)?)?;
    ensure!(value["version"] == 1, "不是受支持的旧版配置");
    value["version"] = 2.into();
    let mut imported: AppState = serde_json::from_value(value)?;
    imported.validate()?;
    let mut warnings = vec![];
    let mut staged = packages::Stage::new(&store.root)?;
    let old_media = source.join("media");
    for a in &mut imported.assets {
        let path = old_media.join(&a.file);
        let bytes = fs::symlink_metadata(&path)?;
        ensure!(
            bytes.is_file() && !bytes.file_type().is_symlink() && bytes.len() <= packages::MAX_FILE,
            "迁移素材无效"
        );
        staged.copy(&path, &a.file)?;
    }
    let mut state = packages::append(
        current,
        &imported.scenes,
        &imported.assets,
        &imported.active_scene_id,
    )?;
    state.settings = imported.settings;
    state.deepseek = imported.deepseek;
    state.migrated_from = Some(origin);
    let ledger_path = source.join("deepseek-ledger.json");
    if ledger_path.exists() {
        let bytes = read_limited(&ledger_path, 16 * 1024 * 1024)?;
        let ledger: pixelferry_services::deepseek::Ledger = serde_json::from_slice(&bytes)?;
        ledger.validate()?;
        if !store.root.join("deepseek-ledger.json").exists() {
            staged.extra("deepseek-ledger.json", &bytes)?;
        } else {
            warnings.push("目标已有消费记录，保留目标记录，未覆盖旧记录".into());
        }
    }
    if source.join("deepseek-key.bin").exists() {
        if store.root.join("deepseek-key.bin").exists() {
            warnings.push("目标已有 Key，保留当前账户".into());
        } else {
            match pixelferry_services::secrets::decrypt_legacy(&source) {
                Ok(key) => staged.extra(
                    "deepseek-key.bin",
                    &pixelferry_services::secrets::encrypt(&key)?,
                )?,
                Err(_) => warnings.push("旧 Key 无法解密，请重新输入；场景和消费记录已迁移".into()),
            }
        }
    }
    state.validate()?;
    staged.commit(store, &state, &imported.assets)?;
    Ok((state, warnings))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backup_recovers_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let s = Storage::new(dir.path()).unwrap();
        let mut state = AppState::default();
        s.save(&state).unwrap();
        state.scenes[0].name = "new".into();
        s.save(&state).unwrap();
        fs::write(dir.path().join("state.json"), b"broken").unwrap();
        let restored = s.load().unwrap();
        assert_eq!(restored.scenes[0].name, "像素启航");
    }
}
