use std::ffi::OsStr;
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
pub(super) const QA_EXPORT_PATH_ENV: &str = "RUVIE_QA_EXPORT_PATH";
const QA_OPEN_EXISTING_PROJECT_ENV: &str = "RUVIE_QA_OPEN_EXISTING_PROJECT";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KnownQaFixture {
    Authoring,
    Audio,
    Path,
}

#[derive(Debug, PartialEq, Eq)]
enum StartupSource {
    DefaultProject,
    BuildFixture {
        fixture: KnownQaFixture,
        save_path: Option<PathBuf>,
    },
    OpenExistingProject(PathBuf),
}

pub(super) fn startup_service(
    plugins: &PluginManager,
) -> Result<(TimelineEditorService, Option<TimelineId>), LibraryError> {
    let fixture_name = unicode_environment_value(QA_FIXTURE_ENV)?;
    let open_existing = unicode_environment_value(QA_OPEN_EXISTING_PROJECT_ENV)?;
    let project_path = std::env::var_os(QA_PROJECT_PATH_ENV);

    match startup_source(
        fixture_name.as_deref(),
        open_existing.as_deref(),
        project_path.as_deref(),
    )? {
        StartupSource::BuildFixture {
            fixture: KnownQaFixture::Authoring,
            save_path,
        } => {
            let media = e2e_media_directory();
            let fixture = library::editor::build_authoring_e2e_fixture(&media, plugins)?;
            install_qa_project_path(&fixture.service, save_path.as_deref())?;
            Ok((fixture.service, Some(fixture.info.timeline_id)))
        }
        StartupSource::BuildFixture {
            fixture: KnownQaFixture::Audio,
            save_path,
        } => {
            let media = e2e_media_directory();
            let fixture = library::editor::build_authoring_audio_e2e_fixture(&media, plugins)?;
            install_qa_project_path(&fixture.service, save_path.as_deref())?;
            Ok((fixture.service, Some(fixture.info.timeline_id)))
        }
        StartupSource::BuildFixture {
            fixture: KnownQaFixture::Path,
            save_path,
        } => {
            let media = e2e_media_directory();
            let fixture = library::editor::build_authoring_path_e2e_fixture(&media, plugins)?;
            install_qa_project_path(&fixture.service, save_path.as_deref())?;
            Ok((fixture.service, Some(fixture.info.timeline_id)))
        }
        StartupSource::OpenExistingProject(path) => {
            let service = TimelineEditorService::open(&path)?;
            let root_timeline_id = service.snapshot()?.root_timeline_id;
            Ok((service, Some(root_timeline_id)))
        }
        StartupSource::DefaultProject => {
            TimelineEditorService::create_default("Untitled Project").map(|service| (service, None))
        }
    }
}

fn unicode_environment_value(name: &str) -> Result<Option<String>, LibraryError> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotUnicode(_)) => Err(LibraryError::Validation(format!(
            "{name} is not valid Unicode"
        ))),
        Err(std::env::VarError::NotPresent) => Ok(None),
    }
}

fn startup_source(
    fixture_name: Option<&str>,
    open_existing_value: Option<&str>,
    project_path: Option<&OsStr>,
) -> Result<StartupSource, LibraryError> {
    let open_existing = match open_existing_value {
        None => false,
        Some("1") => true,
        Some(_) => {
            return Err(LibraryError::Validation(format!(
                "{QA_OPEN_EXISTING_PROJECT_ENV} must be '1' or unset"
            )));
        }
    };

    let Some(fixture_name) = fixture_name else {
        if open_existing {
            return Err(LibraryError::Validation(format!(
                "{QA_OPEN_EXISTING_PROJECT_ENV}=1 requires {QA_FIXTURE_ENV}"
            )));
        }
        return Ok(StartupSource::DefaultProject);
    };
    let fixture = known_qa_fixture(fixture_name)?;
    let project_path = optional_project_path(project_path)?;

    if open_existing {
        return project_path
            .map(StartupSource::OpenExistingProject)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "{QA_OPEN_EXISTING_PROJECT_ENV}=1 requires {QA_PROJECT_PATH_ENV}"
                ))
            });
    }

    Ok(StartupSource::BuildFixture {
        fixture,
        save_path: project_path,
    })
}

fn known_qa_fixture(name: &str) -> Result<KnownQaFixture, LibraryError> {
    match name {
        AUTHORING_E2E_FIXTURE => Ok(KnownQaFixture::Authoring),
        AUTHORING_AUDIO_E2E_FIXTURE => Ok(KnownQaFixture::Audio),
        AUTHORING_PATH_E2E_FIXTURE => Ok(KnownQaFixture::Path),
        _ => Err(LibraryError::Validation(format!(
            "Unknown authoring QA fixture '{name}'"
        ))),
    }
}

fn optional_project_path(path: Option<&OsStr>) -> Result<Option<PathBuf>, LibraryError> {
    match path {
        Some(path) if !path.is_empty() => Ok(Some(PathBuf::from(path))),
        Some(_) => Err(LibraryError::Validation(format!(
            "{QA_PROJECT_PATH_ENV} must not be empty"
        ))),
        None => Ok(None),
    }
}

/// Optional real Project file used by native QA to exercise Save and Open
/// without automating a platform file-picker window.
pub(super) fn qa_project_path() -> Result<Option<PathBuf>, LibraryError> {
    if std::env::var_os(QA_FIXTURE_ENV).is_none() {
        return Ok(None);
    }
    let path = std::env::var_os(QA_PROJECT_PATH_ENV);
    optional_project_path(path.as_deref())
}

/// Optional destination selected by native QA in place of the platform file
/// picker. The returned path still enters the production Export command and
/// worker; this function provides no alternate export endpoint.
pub(super) fn qa_export_path() -> Result<Option<PathBuf>, LibraryError> {
    qa_scoped_path(
        unicode_environment_value(QA_FIXTURE_ENV)?.as_deref(),
        std::env::var_os(QA_EXPORT_PATH_ENV).as_deref(),
        QA_EXPORT_PATH_ENV,
    )
}

fn qa_scoped_path(
    fixture_name: Option<&str>,
    path: Option<&OsStr>,
    path_environment: &str,
) -> Result<Option<PathBuf>, LibraryError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let fixture_name = fixture_name.ok_or_else(|| {
        LibraryError::Validation(format!(
            "{path_environment} is available only with a known {QA_FIXTURE_ENV}"
        ))
    })?;
    known_qa_fixture(fixture_name)?;
    if path.is_empty() {
        return Err(LibraryError::Validation(format!(
            "{path_environment} must not be empty"
        )));
    }
    Ok(Some(PathBuf::from(path)))
}

fn install_qa_project_path(
    service: &TimelineEditorService,
    path: Option<&Path>,
) -> Result<(), LibraryError> {
    if let Some(path) = path {
        service.save_as(path)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_source_defaults_without_qa_environment() {
        assert_eq!(
            startup_source(None, None, None).unwrap(),
            StartupSource::DefaultProject
        );
    }

    #[test]
    fn startup_source_builds_known_fixture_and_preserves_save_path() {
        assert_eq!(
            startup_source(
                Some(AUTHORING_E2E_FIXTURE),
                None,
                Some(OsStr::new("qa-project.ruvie")),
            )
            .unwrap(),
            StartupSource::BuildFixture {
                fixture: KnownQaFixture::Authoring,
                save_path: Some(PathBuf::from("qa-project.ruvie")),
            }
        );
    }

    #[test]
    fn startup_source_opens_existing_project_only_for_known_fixture() {
        assert_eq!(
            startup_source(
                Some(AUTHORING_AUDIO_E2E_FIXTURE),
                Some("1"),
                Some(OsStr::new("saved.ruvie")),
            )
            .unwrap(),
            StartupSource::OpenExistingProject(PathBuf::from("saved.ruvie"))
        );

        let error = startup_source(None, Some("1"), Some(OsStr::new("saved.ruvie")))
            .unwrap_err()
            .to_string();
        assert!(error.contains(QA_FIXTURE_ENV));

        let error = startup_source(Some("unknown"), Some("1"), Some(OsStr::new("saved.ruvie")))
            .unwrap_err()
            .to_string();
        assert!(error.contains("Unknown authoring QA fixture"));
    }

    #[test]
    fn startup_source_requires_project_path_when_opening_existing() {
        let missing = startup_source(Some(AUTHORING_PATH_E2E_FIXTURE), Some("1"), None)
            .unwrap_err()
            .to_string();
        assert!(missing.contains(QA_PROJECT_PATH_ENV));

        let empty = startup_source(
            Some(AUTHORING_PATH_E2E_FIXTURE),
            Some("1"),
            Some(OsStr::new("")),
        )
        .unwrap_err()
        .to_string();
        assert!(empty.contains("must not be empty"));
    }

    #[test]
    fn startup_source_rejects_ambiguous_open_existing_switch_values() {
        let error = startup_source(Some(AUTHORING_E2E_FIXTURE), Some("true"), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("must be '1' or unset"));
    }

    #[test]
    fn qa_export_path_is_available_only_to_a_known_fixture() {
        assert_eq!(
            qa_scoped_path(
                Some(AUTHORING_E2E_FIXTURE),
                Some(OsStr::new("render.mp4")),
                QA_EXPORT_PATH_ENV,
            )
            .unwrap(),
            Some(PathBuf::from("render.mp4"))
        );
        assert_eq!(
            qa_scoped_path(None, None, QA_EXPORT_PATH_ENV).unwrap(),
            None
        );

        let missing_fixture =
            qa_scoped_path(None, Some(OsStr::new("render.mp4")), QA_EXPORT_PATH_ENV)
                .unwrap_err()
                .to_string();
        assert!(missing_fixture.contains(QA_FIXTURE_ENV));

        let unknown_fixture = qa_scoped_path(
            Some("unknown"),
            Some(OsStr::new("render.mp4")),
            QA_EXPORT_PATH_ENV,
        )
        .unwrap_err()
        .to_string();
        assert!(unknown_fixture.contains("Unknown authoring QA fixture"));

        let empty = qa_scoped_path(
            Some(AUTHORING_PATH_E2E_FIXTURE),
            Some(OsStr::new("")),
            QA_EXPORT_PATH_ENV,
        )
        .unwrap_err()
        .to_string();
        assert!(empty.contains("must not be empty"));
    }
}
