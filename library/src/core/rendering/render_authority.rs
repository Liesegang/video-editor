//! External authority snapshot used to invalidate a paused Preview.
//!
//! Terminal frame caching is intentionally absent. A plugin can read an
//! undeclared sidecar, LUT, font, or network resource, so caching final pixels
//! is not sound until the plugin ABI returns a complete dependency manifest.
//! Loader-level caches remain available and own their narrower dependencies.

use std::collections::BTreeSet;

use ruvie_color_management::ExactColorConfigFile;

use crate::model::frame::entity::{FrameContent, FrameItem};
use crate::model::frame::frame::FrameInfo;
use crate::model::project::{ColorConfigIdentity, Project};
use crate::plugin::loaders::FileIdentity;

/// Compact equality token for resources outside the authoritative Project.
///
/// It is recomputed by the Preview while paused. A missing file and an exact
/// OCIO checksum mismatch remain values rather than construction errors, so
/// restoring or replacing the resource advances the render generation.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RenderFrameAuthority {
    plugin_revision: u64,
    resources: Vec<ResourceAuthority>,
}

impl RenderFrameAuthority {
    pub fn capture(project: &Project, frame: &FrameInfo, plugin_revision: u64) -> Self {
        let mut paths = BTreeSet::new();
        collect_frame_paths(&frame.items, &mut paths);
        let mut resources = paths
            .into_iter()
            .map(|path| resource_authority(&path))
            .collect::<Vec<_>>();
        resources.push(color_config_authority(project));
        Self {
            plugin_revision,
            resources,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ResourceAuthority {
    File(FileIdentity),
    Unavailable {
        path: String,
        detail: String,
    },
    BuiltinColorConfig,
    InvalidColorConfig,
    ExactColorConfig {
        path: String,
        expected_sha256: String,
        actual_sha256: Result<String, String>,
    },
}

fn resource_authority(path: &str) -> ResourceAuthority {
    match FileIdentity::read(path) {
        Ok(identity) => ResourceAuthority::File(identity),
        Err(error) => ResourceAuthority::Unavailable {
            path: path.to_string(),
            detail: error.to_string(),
        },
    }
}

fn color_config_authority(project: &Project) -> ResourceAuthority {
    let resolved = project.resolved_color_management();
    let Some(intent) = resolved.model_validated_intent() else {
        return ResourceAuthority::InvalidColorConfig;
    };
    let ColorConfigIdentity::ProjectAsset {
        asset_id, sha256, ..
    } = intent.config().config()
    else {
        return ResourceAuthority::BuiltinColorConfig;
    };
    let Some(asset) = project.assets.iter().find(|asset| asset.id == *asset_id) else {
        return ResourceAuthority::InvalidColorConfig;
    };
    ResourceAuthority::ExactColorConfig {
        path: asset.path.clone(),
        expected_sha256: sha256.clone(),
        actual_sha256: ExactColorConfigFile::read(&asset.path)
            .map(|snapshot| snapshot.sha256().to_string())
            .map_err(|error| error.to_string()),
    }
}

fn collect_frame_paths(items: &[FrameItem], paths: &mut BTreeSet<String>) {
    for item in items {
        match item {
            FrameItem::Object(object) => match &object.content {
                FrameContent::Video { surface, .. } | FrameContent::Image { surface } => {
                    paths.insert(surface.file_path.clone());
                }
                FrameContent::Text { .. }
                | FrameContent::Shape { .. }
                | FrameContent::SkSL { .. } => {}
            },
            FrameItem::Group(group) => collect_frame_paths(&group.items, paths),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::frame::color::Color;
    use crate::model::project::{
        ColorManagementConfig, ExportColorConfig, PreviewColorConfig, PreviewSurfaceEncoding,
    };
    use crate::model::{Asset, AssetKind};
    use ordered_float::OrderedFloat;

    fn empty_frame() -> FrameInfo {
        FrameInfo {
            width: 1,
            height: 1,
            background_color: Color::default(),
            color_profile: "srgb".to_string(),
            render_scale: OrderedFloat(1.0),
            now_time: OrderedFloat(0.0),
            region: None,
            items: Vec::new(),
        }
    }

    #[test]
    fn plugin_revision_participates_in_authority() {
        let project = Project::new("authority");
        let frame = empty_frame();
        assert_ne!(
            RenderFrameAuthority::capture(&project, &frame, 1),
            RenderFrameAuthority::capture(&project, &frame, 2)
        );
    }

    fn project_with_exact_color_config(path: &std::path::Path, bytes: &[u8]) -> Project {
        let mut asset = Asset::new(
            "exact config",
            path.to_str().expect("temporary path is UTF-8"),
            AssetKind::Other,
        );
        let checksum = asset.verify_imported_content(bytes);
        let identity = ColorConfigIdentity::ProjectAsset {
            asset_id: asset.id,
            sha256: checksum,
            ocio_version: "2.5.2".to_string(),
        };
        let mut project = Project::new("exact config authority");
        project.assets.push(asset);
        project
            .set_color_management(
                ColorManagementConfig::new(
                    identity,
                    "fixture-linear",
                    PreviewColorConfig::named_view(
                        "fixture-display",
                        "fixture-view",
                        "fixture-srgb",
                        PreviewSurfaceEncoding::Srgb,
                    ),
                    ExportColorConfig::new("fixture-srgb"),
                )
                .with_srgb_surface_space("fixture-srgb"),
            )
            .expect("exact config model is valid");
        project
    }

    #[test]
    fn preview_color_authority_uses_bounded_snapshot_content_identity() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("preview.ocio");
        let first_bytes = b"aaaaaaaaaaaaaaaa";
        std::fs::write(&path, first_bytes).expect("write first config");
        let project = project_with_exact_color_config(&path, first_bytes);

        let first = color_config_authority(&project);
        std::fs::write(&path, b"bbbbbbbbbbbbbbbb").expect("replace config in place");
        let second = color_config_authority(&project);

        assert_ne!(first, second);
        assert!(matches!(
            first,
            ResourceAuthority::ExactColorConfig {
                expected_sha256,
                actual_sha256: Ok(actual_sha256),
                ..
            } if expected_sha256 == actual_sha256
        ));
        assert!(matches!(
            second,
            ResourceAuthority::ExactColorConfig {
                expected_sha256,
                actual_sha256: Ok(actual_sha256),
                ..
            } if expected_sha256 != actual_sha256
        ));
    }

    #[cfg(unix)]
    #[test]
    fn preview_color_authority_rejects_a_symlink_locator() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("temp directory");
        let target = directory.path().join("target.ocio");
        let link = directory.path().join("link.ocio");
        let bytes = b"exact config";
        std::fs::write(&target, bytes).expect("write target");
        symlink(&target, &link).expect("create symlink");
        let project = project_with_exact_color_config(&link, bytes);

        assert!(matches!(
            color_config_authority(&project),
            ResourceAuthority::ExactColorConfig {
                actual_sha256: Err(detail),
                ..
            } if detail.contains("symlink")
        ));
    }
}
