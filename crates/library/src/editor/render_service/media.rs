//! One media decode/color ingress owner and bounded immutable still-image cache.

use std::collections::VecDeque;

use super::*;
use crate::model::asset::AssetKind;
use crate::model::property::ImageCollectionValue;
use crate::plugin::loaders::FileIdentity;
use crate::rendering::renderer::ManagedImageResource;
use ruvie_color_management::WorkingColorIdentity;

const MAX_MANAGED_IMAGES: usize = 64;
const MAX_MANAGED_IMAGE_BYTES: usize = 256 * 1024 * 1024;

struct ManagedImageEntry {
    file: FileIdentity,
    asset: Asset,
    image: Arc<ManagedImageResource>,
}

#[derive(Default)]
pub(super) struct ManagedImageCache {
    entries: VecDeque<ManagedImageEntry>,
    resident_bytes: usize,
}

impl ManagedImageCache {
    fn get(
        &mut self,
        file: &FileIdentity,
        asset: &Asset,
        working: &WorkingColorIdentity,
    ) -> Option<Arc<ManagedImageResource>> {
        let index = self.entries.iter().position(|entry| {
            &entry.file == file
                && &entry.asset == asset
                && entry.image.image().identity() == working
        })?;
        let entry = self.entries.remove(index)?;
        let image = Arc::clone(&entry.image);
        self.entries.push_back(entry);
        Some(image)
    }

    fn insert(&mut self, file: FileIdentity, asset: Asset, image: Arc<ManagedImageResource>) {
        let bytes = image_bytes(&image);
        // Oversized ordinary media may still render, but may not evict the
        // entire useful cache or exceed its residency budget.
        if bytes > MAX_MANAGED_IMAGE_BYTES {
            return;
        }
        while self.entries.len() >= MAX_MANAGED_IMAGES
            || self.resident_bytes.saturating_add(bytes) > MAX_MANAGED_IMAGE_BYTES
        {
            let Some(entry) = self.entries.pop_front() else {
                break;
            };
            self.resident_bytes = self
                .resident_bytes
                .saturating_sub(image_bytes(&entry.image));
        }
        self.resident_bytes += bytes;
        self.entries
            .push_back(ManagedImageEntry { file, asset, image });
    }
}

fn image_bytes(image: &ManagedImageResource) -> usize {
    std::mem::size_of_val(image.image().pixels().pixels())
}

impl<T: Renderer> RenderService<T> {
    /// Effects/transforms are deliberately outside this boundary: the cache
    /// stores the managed source once, shared by ordinary Images and Sprites.
    pub(super) fn load_media_layer(
        &mut self,
        input: MediaRenderInput<'_>,
        authority: &RenderColorAuthority<'_>,
    ) -> Result<RenderOutput, LibraryError> {
        if let RenderColorAuthority::Managed { assets, pipeline } = authority
            && matches!(input.request, LoadRequest::Image { .. })
            && input.surface.input_color_space.is_none()
            && input.surface.output_color_space.is_none()
        {
            let asset = source_asset_from_assets(assets, input.surface, input.expected_kind)?
                .ok_or_else(|| {
                    LibraryError::Render("Managed Image requires a Project Asset".into())
                })?;
            let image = self.load_managed_image(asset, assets, pipeline)?;
            return Ok(RenderOutput::Working(image.image().clone()));
        }
        self.decode_media_layer(input, authority)
    }

    fn decode_media_layer(
        &self,
        input: MediaRenderInput<'_>,
        authority: &RenderColorAuthority<'_>,
    ) -> Result<RenderOutput, LibraryError> {
        let response = measure_debug(format!("Load {}", input.surface.file_path), || {
            self.plugin_manager
                .load_resource(input.request, &self.cache_manager)
        })?;
        match authority {
            RenderColorAuthority::Managed { assets, pipeline } => ingest_loaded_media_from_assets(
                assets,
                pipeline,
                input.surface,
                input.expected_kind,
                response,
            )
            .map(RenderOutput::Working),
            RenderColorAuthority::UnmanagedAbi => {
                require_unmanaged_abi_srgb(response.decoded(), response.pixels())?;
                Ok(RenderOutput::Image(response.into_rgba8()?))
            }
        }
    }

    fn load_managed_image(
        &mut self,
        asset: &Asset,
        assets: &[Asset],
        pipeline: &ProjectColorPipeline,
    ) -> Result<Arc<ManagedImageResource>, LibraryError> {
        if asset.kind != AssetKind::Image {
            return Err(LibraryError::Validation(format!(
                "Asset {} is not an Image",
                asset.id
            )));
        }
        let contract = pipeline.working_surface_contract();
        let surface = ImageSurface {
            asset_id: Some(asset.id),
            file_path: asset.path.clone(),
            effects: Vec::new(),
            input_color_space: None,
            output_color_space: None,
            transform: Transform::default(),
        };
        let request = LoadRequest::Image {
            path: asset.path.clone(),
        };
        for _ in 0..3 {
            let file = FileIdentity::read(&asset.path)?;
            if let Some(image) = self
                .managed_image_cache
                .get(&file, asset, contract.identity())
            {
                return Ok(image);
            }
            let layer = self.decode_media_layer(
                MediaRenderInput {
                    request: &request,
                    surface: &surface,
                    expected_kind: MediaAssetKind::Image,
                },
                &RenderColorAuthority::Managed { assets, pipeline },
            )?;
            let RenderOutput::Working(image) = layer else {
                return Err(LibraryError::Render(
                    "Managed Image decoder returned an unmanaged layer".into(),
                ));
            };
            if FileIdentity::read(&asset.path)? != file {
                continue;
            }
            // Compute content identity once per source/color cache miss, not
            // once per particle, renderer branch, or rendered frame.
            let image = Arc::new(ManagedImageResource::new(image));
            self.managed_image_cache
                .insert(file, asset.clone(), Arc::clone(&image));
            return Ok(image);
        }
        Err(LibraryError::Render(format!(
            "Image changed repeatedly while loading {:?}",
            asset.path
        )))
    }

    pub(super) fn resolve_point_sprites(
        &mut self,
        collection: &ImageCollectionValue,
        authority: &RenderColorAuthority<'_>,
    ) -> Result<Vec<Arc<ManagedImageResource>>, LibraryError> {
        collection
            .validate()
            .map_err(|error| LibraryError::Validation(error.to_string()))?;
        if collection.assets.is_empty() {
            return Ok(Vec::new());
        }
        let RenderColorAuthority::Managed { assets, pipeline } = authority else {
            return Err(LibraryError::Render(
                "Sprite image collections require Project-managed color authority".into(),
            ));
        };
        let mut images = Vec::with_capacity(collection.assets.len());
        let mut bytes = 0_usize;
        for id in &collection.assets {
            let asset = assets.iter().find(|asset| asset.id == *id).ok_or_else(|| {
                LibraryError::Validation(format!("Sprite Collection references missing Asset {id}"))
            })?;
            let image = self.load_managed_image(asset, assets, pipeline)?;
            bytes = bytes
                .checked_add(image_bytes(&image))
                .filter(|bytes| *bytes <= MAX_MANAGED_IMAGE_BYTES)
                .ok_or_else(|| {
                    LibraryError::Render(
                        "Sprite Collection exceeds the 256 MiB managed image budget".into(),
                    )
                })?;
            images.push(image);
        }
        Ok(images)
    }

    pub(crate) fn preflight_authoring_sprite_collections(
        &mut self,
        project: &AuthoringProject,
        destination: RenderDestination,
        collections: &[ImageCollectionValue],
    ) -> Result<(), LibraryError> {
        let pipeline = self.prepare_authoring_color_pipeline(project, destination)?;
        let authority = RenderColorAuthority::Managed {
            assets: &project.assets,
            pipeline: &pipeline,
        };
        for collection in collections {
            let images = self.resolve_point_sprites(collection, &authority)?;
            self.renderer.preflight_point_sprites(&images)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
