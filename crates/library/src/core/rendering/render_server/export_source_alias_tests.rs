use super::export::require_safe_authoring_output;
use crate::model::authoring::{AuthoringProject, MediaTime, RationalRate};
use crate::model::project::asset::{Asset, AssetKind};
use std::path::Path;

fn project_with_source(path: impl Into<String>, kind: AssetKind) -> AuthoringProject {
    let mut project = AuthoringProject::new(
        "export source alias",
        1,
        1,
        RationalRate::new(24, 1).unwrap(),
        MediaTime::new(1, 1).unwrap(),
    )
    .unwrap();
    let path = path.into();
    project
        .assets
        .push(Asset::new("protected source", &path, kind));
    project
}

fn path_text(path: &Path) -> String {
    path.to_str()
        .expect("temporary test path must be UTF-8")
        .to_string()
}

fn assert_rejected(project: &AuthoringProject, output: &Path) {
    let error = require_safe_authoring_output(project, &path_text(output)).unwrap_err();
    assert!(error.to_string().contains("aliases Asset"), "{error}");
    assert!(
        error.to_string().contains("refusing to overwrite"),
        "{error}"
    );
}

#[test]
fn existing_output_cannot_be_an_asset_source() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.mp4");
    std::fs::write(&source, b"source bytes").unwrap();
    let project = project_with_source(path_text(&source), AssetKind::Video);

    assert_rejected(&project, &source);
    assert_eq!(std::fs::read(source).unwrap(), b"source bytes");
}

#[test]
fn missing_output_cannot_lexically_alias_a_missing_asset_source() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("not-created.png");
    let project = project_with_source(path_text(&source), AssetKind::Image);

    assert_rejected(&project, &source);
    assert!(!source.exists());
}

#[test]
fn relative_asset_and_absolute_output_have_the_same_identity() {
    let current = std::env::current_dir().unwrap().canonicalize().unwrap();
    let directory = tempfile::tempdir_in(&current).unwrap();
    let source = directory.path().join("relative-source.mp4");
    std::fs::write(&source, b"source bytes").unwrap();
    let absolute = source.canonicalize().unwrap();
    let relative = absolute.strip_prefix(&current).unwrap();
    let project = project_with_source(path_text(relative), AssetKind::Video);

    assert_rejected(&project, &absolute);
}

#[cfg(any(unix, windows))]
#[test]
fn hardlink_output_has_the_same_identity_as_its_asset_source() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.mp4");
    let output = directory.path().join("hardlink.mp4");
    std::fs::write(&source, b"source bytes").unwrap();
    std::fs::hard_link(&source, &output).unwrap();
    let project = project_with_source(path_text(&source), AssetKind::Video);

    assert_rejected(&project, &output);
}

#[test]
fn non_media_asset_sources_are_also_protected() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("show.ocio");
    std::fs::write(&config, b"ocio_profile_version: 2\n").unwrap();
    let project = project_with_source(path_text(&config), AssetKind::Other);

    assert_rejected(&project, &config);
}

#[test]
fn generated_asset_without_a_path_does_not_block_export() {
    let directory = tempfile::tempdir().unwrap();
    let project = project_with_source(String::new(), AssetKind::Other);

    require_safe_authoring_output(&project, &path_text(&directory.path().join("safe.png")))
        .unwrap();
}
