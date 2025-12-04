use image;
use image::GenericImageView;
use std::path::{Path, PathBuf};
use std::process::Command;

fn run_renderer_for_frame(project: &Path, frame: u64, plugin_path: Option<&Path>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap();
    
    let project_arg = project.to_str().unwrap().to_string();
    let frames_arg = format!("{}", frame);
    
    let mut cmd = Command::new("cargo");
    cmd.arg("run")
        .arg("--bin")
        .arg("cli")
        .arg("--")
        .arg(project_arg.clone())
        .arg("--frames")
        .arg(frames_arg)
        .current_dir(&workspace_root)
        .env(
            "PATH",
            format!("{}\\target\\debug;{}", std::env::var("PATH").unwrap_or_default(), workspace_root.display())
        );

    if let Some(p) = plugin_path {
        cmd.arg(p.to_str().unwrap());
    }

    let output = cmd.output()?;

    if !output.status.success() {
        return Err(format!(
            "Renderer process failed with: stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ).into());
    }

    let rendered_file = workspace_root.join(format!("rendered/My Composition_{:03}.png", frame));
    if !rendered_file.exists() {
        panic!(
            "Renderer did not produce the expected output file. Looked for: {:?}",
            rendered_file
        );
    }

    Ok(rendered_file)
}

fn compare_images_exact(img1: &image::DynamicImage, img2: &image::DynamicImage) -> bool {
    if img1.dimensions() != img2.dimensions() {
        return false;
    }
    let img1 = img1.to_rgba8();
    let img2 = img2.to_rgba8();
    img1.iter().zip(img2.iter()).all(|(p1, p2)| p1 == p2)
}

fn run_and_compare(project_filename: &str, reference_filename: &str, frame: u64, plugin_filename: Option<&str>) {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap();

    let project_file = workspace_root.join(format!("test_data/{}", project_filename));
    let reference_image = workspace_root.join(format!("test_data/{}", reference_filename));
    
    // Clean up old rendered files before the test
    let rendered_dir = workspace_root.join("rendered");
    if rendered_dir.exists() {
        std::fs::remove_dir_all(&rendered_dir).unwrap();
    }

    let plugin_path = plugin_filename.map(|f| workspace_root.join(format!("target/debug/{}", f)));
    let output_image_path = run_renderer_for_frame(&project_file, frame, plugin_path.as_deref()).unwrap();

    let ref_img = image::open(&reference_image).unwrap();
    let out_img = image::open(&output_image_path).unwrap();

    assert!(compare_images_exact(&ref_img, &out_img), "Images for {} are not exactly equal", project_filename);

    if rendered_dir.exists() {
        std::fs::remove_dir_all(rendered_dir).unwrap();
    }
}

#[test]
#[ignore] // This test now runs the library in a separate process, so ignore by default
fn test_comprehensive_render() {
    run_and_compare("project_comprehensive.json", "reference_comprehensive.png", 0, Some("random_property_plugin.dll"));
}

#[test]
#[ignore]
fn generate_reference_images() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap();
    
    let tests_to_generate = vec![
        ("project_comprehensive.json", "reference_comprehensive.png", 0, Some("random_property_plugin.dll")),
        ("project_easing.json", "reference_easing.png", 15, None),
    ];

    for (project_filename, output_filename, frame, plugin_filename) in tests_to_generate {
        println!("Generating {}...", output_filename);
        let project_file = workspace_root.join(format!("test_data/{}", project_filename));
        let final_path = workspace_root.join(format!("test_data/{}", output_filename));
        
        let plugin_path = plugin_filename.map(|f| workspace_root.join(format!("target/debug/{}", f)));
        let rendered_path = run_renderer_for_frame(&project_file, frame, plugin_path.as_deref()).unwrap();
        std::fs::rename(rendered_path, final_path).unwrap();
    }
}