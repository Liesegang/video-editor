use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use serde_json::json;

fn setup_test_environment() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap();
    let rendered_dir = workspace_root.join("rendered");

    // Clean up old rendered files before the test
    if rendered_dir.exists() {
        fs::remove_dir_all(&rendered_dir).unwrap();
    }
    // Ensure rendered directory exists
    fs::create_dir(&rendered_dir).unwrap(); 

    workspace_root.to_path_buf()
}

fn cleanup_test_environment(workspace_root: &Path) {
    let rendered_dir = workspace_root.join("rendered");
    if rendered_dir.exists() {
        fs::remove_dir_all(&rendered_dir).unwrap();
    }
}

#[test]
#[ignore] // Ignore this test by default, as it needs to be run in a separate process
fn test_video_export() {
    let workspace_root = setup_test_environment();
    let test_data_dir = workspace_root.join("test_data");
    let temp_project_path = test_data_dir.join("temp_video_project.json");
    let output_video_path = workspace_root.join("rendered/My Composition.mp4"); // Assuming composition name is "My Composition"

    // Create a minimal project JSON for video export
    let project_json = json!({
        "name": "My Project",
        "export": {
            "container": "mp4",
            "codec": "h264",
            "pixel_format": "yuv420p" // Common pixel format for h264 mp4
        },
        "compositions": [
            {
                "name": "My Composition",
                "width": 1280,
                "height": 720,
                "background_color": { "r": 0, "g": 0, "b": 255, "a": 255 },
                "color_profile": "srgb",
                "fps": 30,
                "duration": 1.0, // Short duration for fast test
                "tracks": []
            }
        ]
    });

    fs::write(&temp_project_path, project_json.to_string()).unwrap();

    // Run the library as a separate process
    let output = Command::new("cargo")
        .arg("run")
        .arg("--bin")
        .arg("library")
        .arg("--")
        .arg(temp_project_path.to_str().unwrap())
        .current_dir(&workspace_root)
        .output()
        .expect("Failed to execute library process");

    // Check if the command executed successfully
    assert!(output.status.success(), "Library process failed: {:?}", output);
    
    // Verify output
    assert!(output_video_path.exists(), "Output video file does not exist: {:?}", output_video_path);
    assert!(output_video_path.metadata().unwrap().len() > 0, "Output video file is empty");

    // Clean up
    fs::remove_file(&temp_project_path).unwrap();
    cleanup_test_environment(&workspace_root);
}