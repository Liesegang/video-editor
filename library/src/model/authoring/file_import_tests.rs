use crate::editor::TimelineEditorService;
use crate::model::asset::AssetKind;
use crate::plugin::PluginManager;

#[test]
fn file_import_is_authoring_only_and_rejects_duplicate_path() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("payload.unknown");
    std::fs::write(&path, b"authoring asset").expect("fixture");
    let plugins = PluginManager::default();
    let service = TimelineEditorService::create_default("Import").expect("service");

    let (ids, _) = service.import_file(&path, &plugins).expect("import");
    assert_eq!(ids.len(), 1);
    assert!(service.has_asset_with_path(&path).expect("path lookup"));
    assert!(service.import_file(&path, &plugins).is_err());
    service
        .snapshot()
        .expect("snapshot")
        .validate()
        .expect("valid imported project");
}

#[test]
fn uppercase_fbx_import_is_a_fingerprinted_model_asset() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("Triangle.FBX");
    std::fs::write(&path, b"model identity bytes").expect("fixture");
    let plugins = PluginManager::default();
    let service = TimelineEditorService::create_default("FBX Import").expect("service");

    let (ids, _) = service.import_file(&path, &plugins).expect("import");
    let project = service.snapshot().expect("snapshot");
    let asset = project
        .assets
        .iter()
        .find(|asset| ids.contains(&asset.id))
        .expect("imported asset");
    assert_eq!(asset.kind, AssetKind::Model3D);
    assert!(asset.imported_content_sha256().is_some());
}
