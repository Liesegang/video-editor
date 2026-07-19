use anyhow::{Context, Result, ensure};
use image::GenericImageView;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> Result<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("library manifest directory must have a workspace parent")
}

fn renderer_path(workspace_root: &Path) -> Result<std::ffi::OsString> {
    let mut paths = vec![workspace_root.join("target/debug")];
    if let Some(current) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&current));
    }
    std::env::join_paths(paths).context("failed to construct renderer PATH")
}

fn run_renderer_for_frame(
    project: &Path,
    frame: u64,
    plugin_path: Option<&Path>,
) -> Result<PathBuf> {
    let workspace_root = workspace_root()?;
    let mut command = Command::new("cargo");
    command
        .arg("run")
        .arg("--bin")
        .arg("cli")
        .arg("--")
        .arg(project)
        .arg("--frames")
        .arg(frame.to_string())
        .current_dir(&workspace_root)
        .env("PATH", renderer_path(&workspace_root)?);

    if let Some(path) = plugin_path {
        command.arg(path);
    }

    let output = command.output().with_context(|| {
        format!(
            "failed to launch renderer for project {} frame {frame}",
            project.display()
        )
    })?;
    ensure!(
        output.status.success(),
        "renderer failed for project {} frame {frame}: stdout: {}\nstderr: {}",
        project.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let rendered_file = workspace_root.join(format!("rendered/My Composition_{frame:03}.png"));
    ensure!(
        rendered_file.exists(),
        "renderer reported success but did not create {}",
        rendered_file.display()
    );
    Ok(rendered_file)
}

fn compare_images_exact(first: &image::DynamicImage, second: &image::DynamicImage) -> bool {
    if first.dimensions() != second.dimensions() {
        return false;
    }
    let first = first.to_rgba8();
    let second = second.to_rgba8();
    first.iter().zip(second.iter()).all(|(a, b)| a == b)
}

fn run_and_compare(
    project_filename: &str,
    reference_filename: &str,
    frame: u64,
    plugin_filename: Option<&str>,
) -> Result<()> {
    let workspace_root = workspace_root()?;
    let project_file = workspace_root.join("test_data").join(project_filename);
    let reference_image = workspace_root.join("test_data").join(reference_filename);
    let rendered_dir = workspace_root.join("rendered");
    if rendered_dir.exists() {
        std::fs::remove_dir_all(&rendered_dir).with_context(|| {
            format!(
                "failed to clear old render directory {}",
                rendered_dir.display()
            )
        })?;
    }

    let plugin_path =
        plugin_filename.map(|filename| workspace_root.join("target").join("debug").join(filename));
    let output_image = run_renderer_for_frame(&project_file, frame, plugin_path.as_deref())?;
    let reference = image::open(&reference_image).with_context(|| {
        format!(
            "failed to open reference image {}",
            reference_image.display()
        )
    })?;
    let actual = image::open(&output_image)
        .with_context(|| format!("failed to open rendered image {}", output_image.display()))?;
    ensure!(
        compare_images_exact(&reference, &actual),
        "rendered pixels for {project_filename} frame {frame} differ from {reference_filename}"
    );

    if rendered_dir.exists() {
        std::fs::remove_dir_all(&rendered_dir).with_context(|| {
            format!(
                "failed to clean render directory {}",
                rendered_dir.display()
            )
        })?;
    }
    Ok(())
}

#[test]
#[ignore = "runs the renderer in a separate process"]
fn test_comprehensive_render() -> Result<()> {
    run_and_compare(
        "project_comprehensive.json",
        "reference_comprehensive.png",
        0,
        Some("random_property_plugin.dll"),
    )
}

#[test]
#[ignore = "regenerates checked-in reference images"]
fn generate_reference_images() -> Result<()> {
    let workspace_root = workspace_root()?;
    let tests_to_generate = [
        (
            "project_comprehensive.json",
            "reference_comprehensive.png",
            0,
            Some("random_property_plugin.dll"),
        ),
        ("project_easing.json", "reference_easing.png", 15, None),
    ];

    for (project_filename, output_filename, frame, plugin_filename) in tests_to_generate {
        println!("Generating {output_filename}...");
        let project_file = workspace_root.join("test_data").join(project_filename);
        let final_path = workspace_root.join("test_data").join(output_filename);
        let plugin_path = plugin_filename
            .map(|filename| workspace_root.join("target").join("debug").join(filename));
        let rendered_path = run_renderer_for_frame(&project_file, frame, plugin_path.as_deref())?;
        std::fs::rename(&rendered_path, &final_path).with_context(|| {
            format!(
                "failed to move rendered reference {} to {}",
                rendered_path.display(),
                final_path.display()
            )
        })?;
    }
    Ok(())
}
