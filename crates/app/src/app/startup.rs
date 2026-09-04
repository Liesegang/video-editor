use std::path::{Path, PathBuf};

use library::editor::{
    TimelineEditorService, AUTHORING_AUDIO_E2E_FIXTURE, AUTHORING_E2E_FIXTURE,
    AUTHORING_PATH_E2E_FIXTURE,
};
use library::model::authoring::TimelineId;
use library::plugin::PluginManager;
use library::LibraryError;

const QA_FIXTURE_ENV: &str = "RUVIE_QA_FIXTURE";
pub(super) const QA_PROJECT_PATH_ENV: &str = "RUVIE_QA_PROJECT_PATH";

pub(super) fn startup_service(
    plugins: &PluginManager,
) -> Result<(TimelineEditorService, Option<TimelineId>), LibraryError> {
    match std::env::var(QA_FIXTURE_ENV) {
        Ok(name) if name == AUTHORING_E2E_FIXTURE => {
            let media = e2e_media_directory();
            let fixture = library::editor::build_authoring_e2e_fixture(&media, plugins)?;
            install_qa_project_path(&fixture.service)?;
            Ok((fixture.service, Some(fixture.info.timeline_id)))
        }
        Ok(name) if name == AUTHORING_AUDIO_E2E_FIXTURE => {
            let media = e2e_media_directory();
            let fixture = library::editor::build_authoring_audio_e2e_fixture(&media, plugins)?;
            install_qa_project_path(&fixture.service)?;
            Ok((fixture.service, Some(fixture.info.timeline_id)))
        }
        Ok(name) if name == AUTHORING_PATH_E2E_FIXTURE => {
            let media = e2e_media_directory();
            let fixture = library::editor::build_authoring_path_e2e_fixture(&media, plugins)?;
            install_qa_project_path(&fixture.service)?;
            Ok((fixture.service, Some(fixture.info.timeline_id)))
        }
        Ok(name) => Err(LibraryError::Validation(format!(
            "Unknown authoring QA fixture '{name}'"
        ))),
        Err(std::env::VarError::NotUnicode(_)) => Err(LibraryError::Validation(format!(
            "{QA_FIXTURE_ENV} is not valid Unicode"
        ))),
        Err(std::env::VarError::NotPresent) => {
            TimelineEditorService::create_default("Untitled Project").map(|service| (service, None))
        }
    }
}

/// Optional real Project file used by native QA to exercise Save and Open
/// without automating a platform file-picker window.
pub(super) fn qa_project_path() -> Result<Option<PathBuf>, LibraryError> {
    if std::env::var_os(QA_FIXTURE_ENV).is_none() {
        return Ok(None);
    }
    match std::env::var_os(QA_PROJECT_PATH_ENV) {
        Some(path) if !path.is_empty() => Ok(Some(PathBuf::from(path))),
        Some(_) => Err(LibraryError::Validation(format!(
            "{QA_PROJECT_PATH_ENV} must not be empty"
        ))),
        None => Ok(None),
    }
}

fn install_qa_project_path(service: &TimelineEditorService) -> Result<(), LibraryError> {
    if let Some(path) = qa_project_path()? {
        service.save_as(&path)?;
    }
    Ok(())
}

fn e2e_media_directory() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("test_data")
        .join("e2e_media")
}
