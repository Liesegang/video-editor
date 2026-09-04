use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::LibraryError;
use crate::model::asset::{Asset, AssetKind, SourceColorDescription};
use crate::plugin::PluginManager;
use crate::util::local_file::DirectRegularFile;

pub(super) fn probe_assets_for_import(
    path: &Path,
    plugins: &PluginManager,
) -> Result<Vec<Asset>, LibraryError> {
    let opened = DirectRegularFile::open(path)?;
    let canonical_path = opened.canonical_path().to_path_buf();
    let path_string = canonical_path
        .to_str()
        .ok_or_else(|| LibraryError::Validation("Asset path is not valid UTF-8".to_string()))?
        .to_string();
    let base_name = canonical_path
        .file_name()
        .ok_or_else(|| LibraryError::Validation("Asset path has no file name".to_string()))?
        .to_string_lossy()
        .to_string();

    let mut file = opened.into_file();
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let digest = format!("{:x}", hasher.finalize());

    let mut assets = Vec::new();
    if let Some(streams) = plugins.get_available_streams(&path_string)? {
        for stream in streams {
            let suffix = stream
                .stream_index
                .map(|index| format!(" [Stream {index}: {:?}]", stream.kind))
                .unwrap_or_default();
            let mut asset = Asset::new(&format!("{base_name}{suffix}"), &path_string, stream.kind);
            asset.duration = stream.duration;
            asset.fps = stream.fps;
            asset.width = stream.width;
            asset.height = stream.height;
            asset.stream_index = stream.stream_index;
            asset.source_color.replace_detected(stream.source_color);
            if asset.kind == AssetKind::Video {
                asset.frame_count = stream.frame_count;
            }
            asset.record_imported_content_sha256(digest.clone());
            assets.push(asset);
        }
    }

    if assets.is_empty() {
        let (mut kind, duration, fps, width, height, frame_count, source_color) =
            if let Some(metadata) = plugins.get_metadata(&path_string)? {
                (
                    metadata.kind,
                    metadata.duration,
                    metadata.fps,
                    metadata.width,
                    metadata.height,
                    metadata.frame_count,
                    metadata.source_color,
                )
            } else {
                (
                    AssetKind::Other,
                    None,
                    None,
                    None,
                    None,
                    None,
                    SourceColorDescription::default(),
                )
            };
        if kind == AssetKind::Other {
            kind = extension_asset_kind(&canonical_path);
        }
        let mut asset = Asset::new(&base_name, &path_string, kind);
        asset.duration = duration;
        asset.fps = fps;
        asset.width = width;
        asset.height = height;
        asset.source_color.replace_detected(source_color);
        if asset.kind == AssetKind::Video {
            asset.frame_count = frame_count;
        }
        asset.record_imported_content_sha256(digest);
        assets.push(asset);
    }
    Ok(assets)
}

fn extension_asset_kind(path: &Path) -> AssetKind {
    let extension = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    match extension.as_str() {
        "mp4" | "mov" | "avi" | "mkv" | "webm" => AssetKind::Video,
        "png" | "jpg" | "jpeg" | "bmp" | "webp" => AssetKind::Image,
        "mp3" | "wav" | "ogg" | "aac" | "flac" => AssetKind::Audio,
        "obj" | "gltf" | "glb" => AssetKind::Model3D,
        _ => AssetKind::Other,
    }
}
