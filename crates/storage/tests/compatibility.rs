use pixelferry_core::*;
use pixelferry_storage::{self as storage, packages, Storage};
use sha2::{Digest, Sha256};
use std::{fs, io::Write};
fn legacy(root: &std::path::Path) -> AppState {
    fs::create_dir_all(root.join("media")).unwrap();
    let s = AppState::default();
    let mut value = serde_json::to_value(&s).unwrap();
    value["version"] = 1.into();
    for scene in value["scenes"].as_array_mut().unwrap() {
        scene.as_object_mut().unwrap().remove("width");
        scene.as_object_mut().unwrap().remove("height");
    }
    fs::write(root.join("state.json"), serde_json::to_vec(&value).unwrap()).unwrap();
    s
}
#[test]
fn legacy_migration_retains_dimensions_and_preferences_without_touching_source() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("old");
    let old = legacy(&source);
    let before = fs::read(source.join("state.json")).unwrap();
    let store = Storage::new(dir.path().join("new")).unwrap();
    let (next, warnings) = storage::migrate_legacy(&store, &AppState::default(), &source).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(next.scenes.last().unwrap().width, 480);
    assert_eq!(next.deepseek, old.deepseek);
    assert_ne!(next.scenes.last().unwrap().id, old.scenes[0].id);
    assert!(next.bindings.is_empty());
    assert_eq!(fs::read(source.join("state.json")).unwrap(), before);
    assert!(storage::migrate_legacy(&store, &next, &source).is_err());
}
#[test]
fn bad_old_key_does_not_block_scene_migration() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("old");
    legacy(&source);
    fs::write(source.join("deepseek-key.bin"), b"invalid-test-ciphertext").unwrap();
    let store = Storage::new(dir.path().join("new")).unwrap();
    let (next, warnings) = storage::migrate_legacy(&store, &AppState::default(), &source).unwrap();
    assert_eq!(next.scenes.len(), 2);
    assert_eq!(warnings.len(), 1);
    assert!(!store.root.join("deepseek-key.bin").exists());
}
#[test]
fn broken_ledger_rolls_back_all_migration() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("old");
    legacy(&source);
    fs::write(source.join("deepseek-ledger.json"), b"not json").unwrap();
    let store = Storage::new(dir.path().join("new")).unwrap();
    assert!(storage::migrate_legacy(&store, &AppState::default(), &source).is_err());
    assert!(!store.root.join("state.json").exists());
    assert_eq!(fs::read_dir(store.media()).unwrap().count(), 0);
}
fn package_with_asset(root: &std::path::Path, bytes: &[u8], checksum: &str) -> std::path::PathBuf {
    let mut scene = Scene::blank("fixture", 480, 480);
    scene.layers.push(Layer {
        kind: LayerKind::Image,
        asset_id: Some("fixture".into()),
        ..Default::default()
    });
    let manifest = serde_json::json!({"format":"lg600-scene-package","version":1,"createdAt":"2026-10-25T00:00:00Z","scenes":[scene.clone()],"activeSceneId":scene.id,"assets":[{"id":"fixture","name":"fixture","file":"fixture.png","kind":"image","width":16,"height":16,"size":bytes.len(),"sha256":checksum}]});
    let file = root.join("fixture.lg600");
    let mut zip = zip::ZipWriter::new(fs::File::create(&file).unwrap());
    zip.start_file("manifest.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    zip.start_file(
        "media/fixture.png",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(bytes).unwrap();
    zip.finish().unwrap();
    file
}
#[test]
fn bad_checksum_leaves_no_state_or_media() {
    let dir = tempfile::tempdir().unwrap();
    let store = Storage::new(dir.path().join("data")).unwrap();
    let file = package_with_asset(dir.path(), b"fixture", &"0".repeat(64));
    assert!(packages::import(&store, &AppState::default(), &file).is_err());
    assert!(!store.root.join("state.json").exists());
    assert_eq!(fs::read_dir(store.media()).unwrap().count(), 0);
}
#[test]
fn save_failure_removes_committed_new_media() {
    let dir = tempfile::tempdir().unwrap();
    let store = Storage::new(dir.path().join("data")).unwrap();
    fs::create_dir(store.root.join("state.json")).unwrap();
    let bytes = b"fixture";
    let file = package_with_asset(dir.path(), bytes, &format!("{:x}", Sha256::digest(bytes)));
    assert!(packages::import(&store, &AppState::default(), &file).is_err());
    assert_eq!(fs::read_dir(store.media()).unwrap().count(), 0);
}
#[test]
fn legacy_package_references_remapped_media() {
    let dir = tempfile::tempdir().unwrap();
    let store = Storage::new(dir.path().join("data")).unwrap();
    let bytes = b"fixture";
    let file = package_with_asset(dir.path(), bytes, &format!("{:x}", Sha256::digest(bytes)));
    let next = packages::import(&store, &AppState::default(), &file).unwrap();
    let asset = next.assets.last().unwrap();
    assert_ne!(asset.id, "fixture");
    assert_eq!(
        next.scenes.last().unwrap().layers[0].asset_id.as_deref(),
        Some(asset.id.as_str())
    );
    assert_eq!(fs::read(store.media().join(&asset.file)).unwrap(), bytes);
}
#[test]
fn png_and_video_import_and_package_roundtrip() {
    let tools = storage::default_tools();
    assert!(
        tools.join("ffmpeg.exe").exists(),
        "run scripts/prepare-tools.ps1 before media integration tests"
    );
    let dir = tempfile::tempdir().unwrap();
    let store = Storage::new(dir.path().join("data")).unwrap();
    let mut state = AppState::default();
    for (ext, filter) in [
        ("png", "color=c=red:s=16x16:d=0.2"),
        ("mp4", "testsrc=size=32x24:rate=10:duration=0.3"),
    ] {
        let source = dir.path().join(format!("source.{ext}"));
        let mut c = std::process::Command::new(tools.join("ffmpeg.exe"));
        c.args(["-nostdin", "-v", "error", "-f", "lavfi", "-i", filter]);
        if ext == "png" {
            c.args(["-frames:v", "1"]);
        } else {
            c.args(["-pix_fmt", "yuv420p"]);
        }
        c.arg(&source);
        storage::media::run(c, std::time::Duration::from_secs(30)).unwrap();
        let asset = storage::media::import(&source, &store.media(), &tools).unwrap();
        assert!(asset.width > 0 && asset.height > 0);
        if ext == "png" {
            assert_eq!(asset.width, 16);
        }
        let mut layer = Layer::new(if ext == "png" {
            LayerKind::Image
        } else {
            LayerKind::Video
        });
        layer.asset_id = Some(asset.id.clone());
        state.scenes[0].layers.push(layer);
        state.assets.push(asset);
    }
    let path = dir.path().join("media.pixelferry");
    packages::export(&store, &state, &path, true).unwrap();
    let target = Storage::new(dir.path().join("target")).unwrap();
    let next = packages::import(&target, &AppState::default(), &path).unwrap();
    assert_eq!(next.assets.len(), 2);
    for a in &next.assets {
        assert!(target.media().join(&a.file).exists());
    }
}
