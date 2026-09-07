use super::*;
use crate::cache::CacheManager;
use crate::model::authoring::{MediaTime, RationalRate};
use crate::model::frame::color::Color;
use crate::rendering::skia_renderer::SkiaRenderer;

struct ImageFile(std::path::PathBuf);

impl ImageFile {
    fn new(color: [u8; 4]) -> Self {
        let file = Self(
            std::env::temp_dir().join(format!("ruvie-managed-image-{}.png", uuid::Uuid::new_v4())),
        );
        file.write(color);
        file
    }

    fn write(&self, color: [u8; 4]) {
        image::RgbaImage::from_pixel(8, 4, image::Rgba(color))
            .save(&self.0)
            .unwrap();
    }

    fn asset(&self) -> Asset {
        Asset::new("Sprite", &self.0.to_string_lossy(), AssetKind::Image)
    }
}

impl Drop for ImageFile {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.0)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("failed to remove managed Image test source: {error}");
        }
    }
}

fn setup(
    assets: Vec<Asset>,
) -> (
    RenderService<SkiaRenderer>,
    AuthoringProject,
    Arc<ProjectColorPipeline>,
) {
    let mut project = AuthoringProject::new(
        "Sprite images",
        64,
        64,
        RationalRate::new(30, 1).unwrap(),
        MediaTime::new(2, 1).unwrap(),
    )
    .unwrap();
    project.assets = assets;
    let renderer = SkiaRenderer::new(
        64,
        64,
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        },
        false,
        None,
        None,
    )
    .unwrap();
    let mut service = RenderService::new(
        renderer,
        Arc::new(PluginManager::default()),
        Arc::new(CacheManager::new()),
    );
    let pipeline = service
        .prepare_authoring_color_pipeline(&project, RenderDestination::Preview)
        .unwrap();
    (service, project, pipeline)
}

#[test]
fn sprite_images_reuse_immutable_managed_pixels_and_preserve_order() {
    let red = ImageFile::new([255, 0, 0, 128]);
    let green = ImageFile::new([0, 255, 0, 255]);
    let (mut service, project, pipeline) = setup(vec![red.asset(), green.asset()]);
    let authority = RenderColorAuthority::Managed {
        assets: &project.assets,
        pipeline: &pipeline,
    };
    let mut collection = ImageCollectionValue {
        assets: project.assets.iter().map(|asset| asset.id).collect(),
    };
    let first = service
        .resolve_point_sprites(&collection, &authority)
        .unwrap();
    let second = service
        .resolve_point_sprites(&collection, &authority)
        .unwrap();
    assert!(Arc::ptr_eq(&first[0], &second[0]));
    assert!(Arc::ptr_eq(&first[1], &second[1]));
    assert_eq!(first[0].image().pixels().width(), 8);
    assert_eq!(first[0].image().pixels().height(), 4);
    let pixel = first[0].image().pixels().pixels()[0];
    assert!(
        (pixel[0] - pixel[3]).abs() < 0.001,
        "red must be premultiplied once"
    );
    collection.assets.reverse();
    let reordered = service
        .resolve_point_sprites(&collection, &authority)
        .unwrap();
    assert!(Arc::ptr_eq(&first[0], &reordered[1]));
    assert!(Arc::ptr_eq(&first[1], &reordered[0]));
    assert_eq!(service.managed_image_cache.entries.len(), 2);
}

#[test]
fn replacing_or_deleting_a_source_cannot_reuse_stale_sprite_pixels() {
    let file = ImageFile::new([255, 0, 0, 255]);
    let (mut service, project, pipeline) = setup(vec![file.asset()]);
    let asset = &project.assets[0];
    let first = service
        .load_managed_image(asset, &project.assets, &pipeline)
        .unwrap();
    // A new file identity, not a sleep or timestamp-resolution assumption.
    let replacement = ImageFile::new([0, 255, 0, 255]);
    std::fs::rename(&replacement.0, &file.0).unwrap();
    let second = service
        .load_managed_image(asset, &project.assets, &pipeline)
        .unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
    assert_ne!(
        first.image().pixels().pixels(),
        second.image().pixels().pixels()
    );
    std::fs::remove_file(&file.0).unwrap();
    assert!(
        service
            .load_managed_image(asset, &project.assets, &pipeline)
            .is_err()
    );
}

#[test]
fn source_color_authority_change_is_not_hidden_by_a_managed_image_cache_hit() {
    let file = ImageFile::new([128, 0, 0, 255]);
    let (mut service, mut project, pipeline) = setup(vec![file.asset()]);
    service
        .load_managed_image(&project.assets[0], &project.assets, &pipeline)
        .unwrap();
    // The source metadata now demands precision the encoded file cannot
    // supply. The current ingress owner must reject it, not serve old pixels.
    project.assets[0]
        .source_color
        .replace_detected(crate::model::asset::SourceColorDescription {
            bit_depth: Some(16),
            ..Default::default()
        });
    assert!(
        service
            .load_managed_image(&project.assets[0], &project.assets, &pipeline)
            .is_err()
    );
}

#[test]
fn nonempty_collections_fail_without_project_authority_or_valid_image_assets() {
    let (mut service, project, pipeline) = setup(Vec::new());
    let collection = ImageCollectionValue {
        assets: vec![uuid::Uuid::new_v4()],
    };
    assert!(
        service
            .resolve_point_sprites(&collection, &RenderColorAuthority::UnmanagedAbi)
            .is_err()
    );
    assert!(
        service
            .resolve_point_sprites(
                &collection,
                &RenderColorAuthority::Managed {
                    assets: &project.assets,
                    pipeline: &pipeline
                }
            )
            .is_err()
    );
    assert!(
        service
            .resolve_point_sprites(
                &ImageCollectionValue::default(),
                &RenderColorAuthority::UnmanagedAbi
            )
            .unwrap()
            .is_empty()
    );
}

#[test]
fn managed_image_lru_has_bounded_entries_and_does_not_revoke_borrowed_resources() {
    let file = ImageFile::new([255, 255, 255, 255]);
    let (mut service, project, pipeline) = setup(vec![file.asset()]);
    let asset = &project.assets[0];
    let image = service
        .load_managed_image(asset, &project.assets, &pipeline)
        .unwrap();
    let identity = FileIdentity::read(&asset.path).unwrap();
    for _ in 0..(MAX_MANAGED_IMAGES + 2) {
        let mut distinct = asset.clone();
        distinct.id = uuid::Uuid::new_v4();
        service
            .managed_image_cache
            .insert(identity.clone(), distinct, Arc::clone(&image));
    }
    assert_eq!(
        service.managed_image_cache.entries.len(),
        MAX_MANAGED_IMAGES
    );
    assert_eq!(
        service.managed_image_cache.resident_bytes,
        MAX_MANAGED_IMAGES * image_bytes(&image)
    );
    assert_eq!(image.image().pixels().pixels()[0], [1.0; 4]);
}
