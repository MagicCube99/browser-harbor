use std::{fs, path::PathBuf};

use browser_harbor::{
    app::AppState,
    config::ConfigStore,
    domain::{settings::Settings, target::TargetId},
};
use tempfile::tempdir;

#[test]
fn legacy_config_compacts_then_persists_a_target_switch() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("manager.json");
    let mut settings = Settings {
        schema_version: 5,
        selected_browser: TargetId::Edge,
        ..Settings::default()
    };
    let mut value = serde_json::to_value(&settings).unwrap();
    value["targets"]
        .as_object_mut()
        .unwrap()
        .retain(|target, _| target == "edge");
    fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

    let store = ConfigStore::new(path.clone());
    settings = store.load().unwrap();
    assert!(store.compact_if_needed(&settings).unwrap());

    let mut state = AppState::with_tool_root(
        settings,
        ConfigStore::new(path.clone()),
        directory.path().to_path_buf(),
    );
    state.select(TargetId::SkyKakapo);
    state.save().unwrap();

    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["schemaVersion"], 10);
    assert_eq!(saved["selectedBrowser"], "sky_kakapo");

    let reloaded = ConfigStore::new(path).load().unwrap();
    let sky = &reloaded.targets[&TargetId::SkyKakapo];
    assert_eq!(sky.browser_root, Some(PathBuf::from(r".\SkyKakapo")));
    assert_eq!(
        sky.browser_directory,
        Some(PathBuf::from(r".\SkyKakapo\App"))
    );
    assert_eq!(
        sky.profile_directory,
        Some(PathBuf::from(r".\SkyKakapo\Profiles"))
    );
}
