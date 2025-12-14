use library::loader::video::VideoReader;
use std::path::PathBuf;

fn get_test_file_path(filename: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // Go up to project root
    path.push("test_data");
    path.push(filename);
    path
}

#[test]
fn test_video_reader_creation() {
    let path = get_test_file_path("test.mp4");
    println!("Test file path: {:?}", path);
    assert!(path.exists(), "Test file test.mp4 does not exist");

    let reader = VideoReader::new(path.to_str().unwrap());
    assert!(
        reader.is_ok(),
        "Failed to create VideoReader: {:?}",
        reader.err()
    );
}

#[test]
fn test_video_reader_metadata() {
    let path = get_test_file_path("test.mp4");
    let reader = VideoReader::new(path.to_str().unwrap()).expect("Failed to create VideoReader");

    // Check FPS
    let fps = reader.get_fps();
    println!("FPS: {}", fps);
    assert!(fps > 0.0, "FPS should be positive");
    // assert!((fps - 30.0).abs() < 1.0, "FPS should be around 30");

    // Check Dimensions
    let (width, height) = reader.get_dimensions();
    println!("Dimensions: {}x{}", width, height);
    assert!(width > 0);
    assert!(height > 0);

    // Check Duration
    let duration = reader.get_duration();
    println!("Duration: {:?}", duration);
    assert!(duration.is_some());
    assert!(duration.unwrap() > 0.0);
}

#[test]
fn test_video_reader_decode_frame() {
    let path = get_test_file_path("test.mp4");
    let mut reader =
        VideoReader::new(path.to_str().unwrap()).expect("Failed to create VideoReader");

    // Decode frame 0
    let frame0 = reader.decode_frame(0);
    assert!(frame0.is_ok(), "Failed to decode frame 0");
    let img0 = frame0.unwrap();
    assert_eq!(img0.width, reader.get_dimensions().0);
    assert_eq!(img0.height, reader.get_dimensions().1);
    assert!(!img0.data.is_empty());

    // Decode frame 30 (1 sec in)
    let frame30 = reader.decode_frame(30);
    assert!(frame30.is_ok(), "Failed to decode frame 30");
    let img30 = frame30.unwrap();
    assert_eq!(img30.width, reader.get_dimensions().0);
    assert!(!img30.data.is_empty());
}
