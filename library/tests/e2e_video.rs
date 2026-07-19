use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn setup_test_environment() -> Result<PathBuf> {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("library manifest directory must have a workspace parent")?;
    let rendered_dir = workspace_root.join("rendered");
    if rendered_dir.exists() {
        fs::remove_dir_all(&rendered_dir).with_context(|| {
            format!(
                "failed to clear render directory {}",
                rendered_dir.display()
            )
        })?;
    }
    fs::create_dir(&rendered_dir).with_context(|| {
        format!(
            "failed to create render directory {}",
            rendered_dir.display()
        )
    })?;
    Ok(workspace_root)
}

fn cleanup_test_environment(workspace_root: &Path) -> Result<()> {
    let rendered_dir = workspace_root.join("rendered");
    if rendered_dir.exists() {
        fs::remove_dir_all(&rendered_dir).with_context(|| {
            format!(
                "failed to remove render directory {}",
                rendered_dir.display()
            )
        })?;
    }
    Ok(())
}

#[test]
#[ignore = "runs the exporter in a separate process"]
fn test_video_export() -> Result<()> {
    let workspace_root = setup_test_environment()?;
    let temp_project_path = workspace_root.join("test_data/temp_video_project.json");
    let output_video_path = workspace_root.join("rendered/My Composition.mp4");

    let project_json = json!({
        "name": "My Project",
        "export": {
            "container": "mp4",
            "codec": "h264",
            "pixel_format": "yuv420p"
        },
        "compositions": [
            {
                "name": "My Composition",
                "width": 1280,
                "height": 720,
                "background_color": { "r": 0, "g": 0, "b": 255, "a": 255 },
                "color_profile": "srgb",
                "fps": 30,
                "duration": 1.0,
                "tracks": []
            }
        ]
    });
    fs::write(&temp_project_path, project_json.to_string()).with_context(|| {
        format!(
            "failed to write temporary project {}",
            temp_project_path.display()
        )
    })?;

    let output = Command::new("cargo")
        .arg("run")
        .arg("--bin")
        .arg("library")
        .arg("--")
        .arg(&temp_project_path)
        .current_dir(&workspace_root)
        .output()
        .context("failed to launch the library export process")?;
    ensure!(
        output.status.success(),
        "library export process failed: stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    ensure!(
        output_video_path.exists(),
        "export process did not create {}",
        output_video_path.display()
    );
    let output_size = output_video_path
        .metadata()
        .with_context(|| {
            format!(
                "failed to inspect exported video {}",
                output_video_path.display()
            )
        })?
        .len();
    ensure!(output_size > 0, "exported video is empty");

    fs::remove_file(&temp_project_path).with_context(|| {
        format!(
            "failed to remove temporary project {}",
            temp_project_path.display()
        )
    })?;
    cleanup_test_environment(&workspace_root)
}
