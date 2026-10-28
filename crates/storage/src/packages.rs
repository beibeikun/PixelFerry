use crate::Storage;
use anyhow::{ensure, Context, Result};
use pixelferry_core::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};
pub const MAX_FILE: u64 = 1024 * 1024 * 1024;
pub const MAX_TOTAL: u64 = 2 * MAX_FILE;
const MAX_MANIFEST: u64 = 4 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    format: String,
    version: u32,
    created_at: String,
    scenes: Vec<Scene>,
    active_scene_id: SceneId,
    assets: Vec<PackageAsset>,
}
#[derive(Serialize, Deserialize)]
struct PackageAsset {
    #[serde(flatten)]
    asset: Asset,
    size: u64,
    sha256: String,
}
fn hash(path: &Path) -> Result<(u64, String)> {
    let m = fs::symlink_metadata(path)?;
    ensure!(
        m.is_file() && !m.file_type().is_symlink() && m.len() <= MAX_FILE,
        "素材类型或大小无效"
    );
    let mut f = fs::File::open(path)?;
    let mut bytes = 0;
    let mut h = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = f.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        ensure!(bytes <= MAX_FILE, "素材超限");
        h.update(&buffer[..n]);
    }
    Ok((bytes, format!("{:x}", h.finalize())))
}
pub fn export(store: &Storage, state: &AppState, destination: &Path, all: bool) -> Result<()> {
    state.validate()?;
    let scenes: Vec<_> = state
        .scenes
        .iter()
        .filter(|s| all || s.id == state.active_scene_id)
        .cloned()
        .collect();
    let referenced: BTreeSet<_> = scenes
        .iter()
        .flat_map(|s| s.layers.iter().filter_map(|l| l.asset_id.as_ref()))
        .collect();
    let mut assets = vec![];
    let mut total = 0;
    for a in state.assets.iter().filter(|a| referenced.contains(&a.id)) {
        let (size, sha256) = hash(&store.media().join(&a.file))?;
        total += size;
        ensure!(total <= MAX_TOTAL, "素材总大小超限");
        assets.push(PackageAsset {
            asset: a.clone(),
            size,
            sha256,
        });
    }
    let manifest = Manifest {
        format: "pixelferry-scene-package".into(),
        version: 1,
        created_at: chrono::Utc::now().to_rfc3339(),
        scenes,
        active_scene_id: state.active_scene_id.clone(),
        assets,
    };
    let json = serde_json::to_vec(&manifest)?;
    ensure!(json.len() as u64 <= MAX_MANIFEST, "清单过大");
    let temp = destination.with_extension(format!("tmp-{}", new_id()));
    let result = (|| -> Result<()> {
        let file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        let mut zip = ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("manifest.json", options)?;
        zip.write_all(&json)?;
        for a in &manifest.assets {
            zip.start_file(format!("media/{}", a.asset.file), options)?;
            std::io::copy(
                &mut fs::File::open(store.media().join(&a.asset.file))?,
                &mut zip,
            )?;
        }
        zip.finish()?.sync_all()?;
        replace(&temp, destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
fn replace(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        let w = |p: &Path| {
            p.to_string_lossy()
                .encode_utf16()
                .chain(Some(0))
                .collect::<Vec<_>>()
        };
        ensure!(
            unsafe {
                MoveFileExW(
                    w(source).as_ptr(),
                    w(destination).as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            } != 0,
            "文件提交失败"
        );
    }
    #[cfg(not(windows))]
    fs::rename(source, destination)?;
    Ok(())
}
pub fn import(store: &Storage, current: &AppState, source: &Path) -> Result<AppState> {
    let mut zip = ZipArchive::new(fs::File::open(source)?)?;
    ensure!(zip.len() <= MAX_ASSETS + 2, "场景包文件过多");
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let f = zip.by_index(i)?;
        let name = f.name();
        ensure!(names.insert(name.to_lowercase()), "包内路径重复");
        ensure!(
            f.unix_mode().is_none_or(|mode| mode & 0o170000 != 0o120000),
            "拒绝符号链接"
        );
        ensure!(
            name == "manifest.json"
                || name == "media/"
                || name.strip_prefix("media/").is_some_and(valid_media_file),
            "包内路径不允许"
        );
        ensure!(f.size() <= MAX_FILE, "单个文件超限");
        total = total.checked_add(f.size()).context("解压大小溢出")?;
        ensure!(total <= MAX_TOTAL + MAX_MANIFEST, "解压总大小超限");
    }
    let manifest: Manifest = {
        let mut entry = zip.by_name("manifest.json")?;
        ensure!(entry.size() <= MAX_MANIFEST, "清单超限");
        let mut bytes = vec![];
        (&mut entry)
            .take(MAX_MANIFEST + 1)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() as u64 <= MAX_MANIFEST, "清单超限");
        serde_json::from_slice(&bytes)?
    };
    ensure!(
        ["pixelferry-scene-package", "lg600-scene-package"].contains(&manifest.format.as_str())
            && manifest.version == 1,
        "场景包版本不支持"
    );
    ensure!(
        !manifest.scenes.is_empty()
            && manifest.scenes.len() <= MAX_SCENES
            && manifest.assets.len() <= MAX_ASSETS,
        "包内数量超限"
    );
    let assets: Vec<_> = manifest.assets.iter().map(|a| a.asset.clone()).collect();
    let candidate = AppState {
        scenes: manifest.scenes.clone(),
        active_scene_id: manifest.active_scene_id.clone(),
        assets: assets.clone(),
        bindings: vec![],
        ..AppState::default()
    };
    candidate.validate()?;
    let allowed: BTreeSet<_> = manifest
        .assets
        .iter()
        .map(|a| format!("media/{}", a.asset.file).to_lowercase())
        .chain(["manifest.json".into(), "media/".into()])
        .collect();
    ensure!(names.is_subset(&allowed), "包内含未声明文件");
    let mut stage = Stage::new(&store.root)?;
    for a in &manifest.assets {
        ensure!(
            a.size <= MAX_FILE
                && a.sha256.len() == 64
                && a.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "素材清单无效"
        );
        let mut entry = zip.by_name(&format!("media/{}", a.asset.file))?;
        ensure!(entry.size() == a.size, "素材大小不匹配");
        let mut dest = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(stage.path.join(&a.asset.file))?;
        let copied = std::io::copy(&mut (&mut entry).take(a.size + 1), &mut dest)?;
        ensure!(copied == a.size, "素材长度无效");
        dest.sync_all()?;
        let (size, actual) = hash(&stage.path.join(&a.asset.file))?;
        ensure!(
            size == a.size && actual == a.sha256.to_lowercase(),
            "素材 SHA-256 校验失败"
        );
    }
    let next = append(
        current,
        &manifest.scenes,
        &assets,
        &manifest.active_scene_id,
    )?;
    stage.commit(store, &next, &assets)?;
    Ok(next)
}
/// Remap all IDs, preserve source files through a deterministic old-file -> new-file map.
pub fn append(
    current: &AppState,
    scenes: &[Scene],
    assets: &[Asset],
    active: &SceneId,
) -> Result<AppState> {
    ensure!(
        current.scenes.len() + scenes.len() <= MAX_SCENES
            && current.assets.len() + assets.len() <= MAX_ASSETS,
        "导入后数量超限"
    );
    let mut next = current.clone();
    let mut asset_ids = BTreeMap::new();
    for a in assets {
        let mut a = a.clone();
        let old = a.id.clone();
        a.id = new_id();
        asset_ids.insert(old, a.id.clone());
        let ext = if a.kind == "video" { "mp4" } else { "png" };
        a.file = format!("{}.{ext}", a.id);
        next.assets.push(a);
    }
    let mut used: BTreeSet<_> = next.scenes.iter().map(|s| s.name.clone()).collect();
    for s in scenes {
        let old = s.id.clone();
        let mut s = s.clone();
        s.id = SceneId(new_id());
        while used.contains(&s.name) {
            s.name.push_str("（导入）");
        }
        used.insert(s.name.clone());
        for l in &mut s.layers {
            l.id = new_id();
            if let Some(id) = &l.asset_id {
                l.asset_id = Some(asset_ids.get(id).context("素材引用无效")?.clone());
            }
        }
        if &old == active {
            next.active_scene_id = s.id.clone();
        }
        next.scenes.push(s);
    }
    next.validate()?;
    Ok(next)
}
pub(crate) struct Stage {
    path: PathBuf,
}
impl Stage {
    pub fn new(root: &Path) -> Result<Self> {
        let path = root.join(format!(".import-{}", new_id()));
        fs::create_dir(&path)?;
        Ok(Self { path })
    }
    pub fn copy(&mut self, source: &Path, file: &str) -> Result<()> {
        ensure!(valid_media_file(file), "素材路径无效");
        fs::copy(source, self.path.join(file))?;
        Ok(())
    }
    pub fn extra(&mut self, file: &str, bytes: &[u8]) -> Result<()> {
        ensure!(
            ["deepseek-key.bin", "deepseek-ledger.json"].contains(&file),
            "迁移文件不允许"
        );
        fs::write(self.path.join(file), bytes)?;
        Ok(())
    }
    pub fn commit(
        &mut self,
        store: &Storage,
        state: &AppState,
        source_assets: &[Asset],
    ) -> Result<()> {
        let mut added = vec![];
        let result = (|| -> Result<()> {
            let new_assets = &state.assets[state.assets.len() - source_assets.len()..];
            for (old, new) in source_assets.iter().zip(new_assets) {
                let dest = store.media().join(&new.file);
                ensure!(!dest.exists(), "素材文件冲突");
                fs::rename(self.path.join(&old.file), &dest)?;
                added.push(dest);
            }
            for name in ["deepseek-key.bin", "deepseek-ledger.json"] {
                let source = self.path.join(name);
                if source.exists() {
                    let dest = store.root.join(name);
                    ensure!(!dest.exists(), "迁移文件已存在");
                    fs::rename(source, &dest)?;
                    added.push(dest);
                }
            }
            store.save(state)?;
            Ok(())
        })();
        if result.is_err() {
            for path in added {
                let _ = fs::remove_file(path);
            }
        }
        result
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_roundtrip_remaps_ids() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage::new(dir.path()).unwrap();
        let state = AppState::default();
        let file = dir.path().join("test.pixelferry");
        export(&store, &state, &file, true).unwrap();
        let next = import(&store, &state, &file).unwrap();
        assert_eq!(next.scenes.len(), 2);
        assert_ne!(next.scenes[0].id, next.scenes[1].id);
        assert_eq!(next.scenes[1].width, 480);
    }
    #[test]
    fn mixed_text_survives_save_load_and_package_import_without_sampling_data() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage::new(dir.path()).unwrap();
        let mut state = AppState::default();
        let mut layer = Layer::new(LayerKind::Text);
        layer.text = "余额：[DeepSeek · 余额]".into();
        layer.vertical_align = "bottom".into();
        let rich = RichText {
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
                    affix_scale: Some(0.7),
                    style: RichStyle {
                        font_size: Some(48.),
                        color: Some("#ffcc00".into()),
                        ..Default::default()
                    },
                },
                RichRun::Sensor {
                    sensor_id: "afterburner:stable:0".into(),
                    label: "GPU 温度".into(),
                    decimals: 1,
                    show_unit: true,
                    affix_scale: Some(0.5),
                    style: RichStyle::default(),
                },
            ],
        };
        layer.rich_text = Some(rich.clone());
        state.scenes[0].layers.push(layer);
        store.save(&state).unwrap();
        assert_eq!(
            store.load().unwrap().scenes[0]
                .layers
                .last()
                .unwrap()
                .rich_text
                .as_ref(),
            Some(&rich)
        );
        let file = dir.path().join("mixed.pixelferry");
        export(&store, &state, &file, true).unwrap();
        let imported = import(&store, &state, &file).unwrap();
        assert_eq!(
            imported
                .scenes
                .last()
                .unwrap()
                .layers
                .last()
                .unwrap()
                .vertical_align,
            "bottom"
        );
        assert_eq!(
            imported
                .scenes
                .last()
                .unwrap()
                .layers
                .last()
                .unwrap()
                .rich_text
                .as_ref(),
            Some(&rich)
        );
    }
    #[test]
    fn rejects_traversal_without_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage::new(dir.path()).unwrap();
        let file = dir.path().join("bad.zip");
        let mut z = ZipWriter::new(fs::File::create(&file).unwrap());
        z.start_file("../escape", SimpleFileOptions::default())
            .unwrap();
        z.write_all(b"bad").unwrap();
        z.finish().unwrap();
        let state = AppState::default();
        assert!(import(&store, &state, &file).is_err());
        assert!(!dir.path().join("state.json").exists());
    }
    #[test]
    fn package_excludes_accounts_and_bindings() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage::new(dir.path()).unwrap();
        let file = dir.path().join("test.pixelferry");
        export(&store, &AppState::default(), &file, true).unwrap();
        let mut z = ZipArchive::new(fs::File::open(file).unwrap()).unwrap();
        let mut bytes = String::new();
        z.by_name("manifest.json")
            .unwrap()
            .read_to_string(&mut bytes)
            .unwrap();
        assert!(!bytes.contains("bindings") && !bytes.contains("deepseek-key"));
    }
}
