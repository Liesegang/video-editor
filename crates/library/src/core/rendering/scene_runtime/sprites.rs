//! Managed Sprite atlas resources shared by Particle and procedural Points.

use std::collections::HashMap;
use std::sync::Arc;

use bytemuck::cast_slice;
use glow::HasContext;
use sha2::{Digest, Sha256};

use crate::error::LibraryError;
use crate::model::property::IMAGE_COLLECTION_MAX_ASSETS;
use crate::rendering::renderer::ManagedImageResource;

use super::{SceneRuntime, drain_gl_errors};

const ATLAS_GUTTER: u32 = 1;
const MAX_CACHED_ATLASES: usize = 16;
const MAX_ATLAS_CACHE_BYTES: u64 = 256 * 1024 * 1024;
const RGBA32F_BYTES_PER_PIXEL: u64 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct SpriteAtlasKey([u8; 32]);

#[derive(Clone, Copy, Debug)]
pub(super) struct SpriteAtlasEntry {
    pub uv: [f32; 4],
    pub aspect: [f32; 2],
}

#[derive(Clone)]
pub(super) struct SpriteAtlas {
    pub texture: glow::Texture,
    pub entries: Vec<SpriteAtlasEntry>,
    pub byte_len: u64,
    pub last_used: u64,
}

impl SpriteAtlas {
    fn create(
        gl: &glow::Context,
        sprites: &[Arc<ManagedImageResource>],
        max_texture_size: u32,
        last_used: u64,
    ) -> Result<Self, LibraryError> {
        let layout = AtlasLayout::derive(sprites, max_texture_size)?;
        let upload_bytes = u64::from(layout.width)
            .checked_mul(u64::from(layout.height))
            .and_then(|pixels| pixels.checked_mul(RGBA32F_BYTES_PER_PIXEL))
            .ok_or_else(|| LibraryError::Render("Sprite atlas upload size overflow".into()))?;
        if upload_bytes > MAX_ATLAS_CACHE_BYTES {
            return Err(LibraryError::Render(format!(
                "Sprite atlas upload requires {upload_bytes} bytes, exceeding the {MAX_ATLAS_CACHE_BYTES}-byte limit"
            )));
        }
        let pixels = layout.pixels(sprites)?;
        // SAFETY: SceneRuntime exclusively owns the current context. The new
        // texture remains local until every allocation/upload check succeeds.
        let texture = unsafe { gl.create_texture() }.map_err(|error| {
            LibraryError::Render(format!("Cannot create GPU Sprite atlas: {error}"))
        })?;
        // SAFETY: `texture` is live in this current context; the upload slice
        // exactly covers the checked RGBA32F atlas dimensions. SavedGlState
        // restores every unpack and texture-unit binding changed here.
        unsafe {
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
            gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
            gl.pixel_store_i32(glow::UNPACK_IMAGE_HEIGHT, 0);
            gl.pixel_store_i32(glow::UNPACK_SKIP_ROWS, 0);
            gl.pixel_store_i32(glow::UNPACK_SKIP_PIXELS, 0);
            gl.pixel_store_i32(glow::UNPACK_SKIP_IMAGES, 0);
            gl.pixel_store_i32(glow::UNPACK_SWAP_BYTES, 0);
            gl.pixel_store_i32(glow::UNPACK_LSB_FIRST, 0);
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA32F as i32,
                layout.width as i32,
                layout.height as i32,
                0,
                glow::RGBA,
                glow::FLOAT,
                glow::PixelUnpackData::Slice(Some(cast_slice(&pixels))),
            );
        }
        let errors = drain_gl_errors(gl);
        if !errors.is_empty() {
            // SAFETY: the failed resource has not entered the cache.
            unsafe { gl.delete_texture(texture) };
            return Err(LibraryError::Render(format!(
                "GPU Sprite atlas upload failed (OpenGL errors {})",
                errors
                    .iter()
                    .map(|error| format!("0x{error:04x}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        Ok(Self {
            texture,
            entries: layout.entries,
            byte_len: u64::from(layout.width) * u64::from(layout.height) * RGBA32F_BYTES_PER_PIXEL,
            last_used,
        })
    }

    pub fn destroy(self, gl: &glow::Context) {
        // SAFETY: the cache entry uniquely owns this texture in this context.
        unsafe { gl.delete_texture(self.texture) };
    }
}

impl SceneRuntime {
    pub(super) fn prepare_sprite_atlas(
        &mut self,
        sprites: &[Arc<ManagedImageResource>],
        use_tick: u64,
    ) -> Result<Option<SpriteAtlas>, LibraryError> {
        if sprites.is_empty() {
            return Ok(None);
        }
        if sprites.len() > IMAGE_COLLECTION_MAX_ASSETS {
            return Err(LibraryError::Validation(format!(
                "Sprite collection supports at most {IMAGE_COLLECTION_MAX_ASSETS} images"
            )));
        }
        let key = atlas_key(sprites);
        if let Some(atlas) = self.sprite_atlases.get_mut(&key) {
            atlas.last_used = use_tick;
            return Ok(Some(atlas.clone()));
        }
        let max_texture_size = self
            .capability
            .as_ref()
            .map_err(|diagnostic| LibraryError::Render(diagnostic.clone()))?
            .max_texture_size;
        // Allocate before mutating the cache. A failed replacement leaves all
        // previously usable collections and Particle invocation state intact.
        let atlas = SpriteAtlas::create(&self.gl, sprites, max_texture_size, use_tick)?;
        self.sprite_atlases.insert(key, atlas.clone());
        self.evict_sprite_atlases(key);
        Ok(Some(atlas))
    }

    fn evict_sprite_atlases(&mut self, protected: SpriteAtlasKey) {
        while self.sprite_atlases.len() > MAX_CACHED_ATLASES
            || sprite_cache_bytes(&self.sprite_atlases) > MAX_ATLAS_CACHE_BYTES
        {
            let candidate = self
                .sprite_atlases
                .iter()
                .filter(|(key, _)| **key != protected)
                .min_by_key(|(_, atlas)| atlas.last_used)
                .map(|(key, _)| *key);
            let Some(candidate) = candidate else {
                break;
            };
            if let Some(atlas) = self.sprite_atlases.remove(&candidate) {
                atlas.destroy(&self.gl);
            }
        }
    }
}

fn atlas_key(sprites: &[Arc<ManagedImageResource>]) -> SpriteAtlasKey {
    let mut digest = Sha256::new();
    digest.update((sprites.len() as u64).to_le_bytes());
    for sprite in sprites {
        digest.update(sprite.fingerprint());
    }
    SpriteAtlasKey(digest.finalize().into())
}

fn sprite_cache_bytes(atlases: &HashMap<SpriteAtlasKey, SpriteAtlas>) -> u64 {
    atlases
        .values()
        .fold(0_u64, |bytes, atlas| bytes.saturating_add(atlas.byte_len))
}

struct AtlasLayout {
    width: u32,
    height: u32,
    origins: Vec<[u32; 2]>,
    entries: Vec<SpriteAtlasEntry>,
}

impl AtlasLayout {
    fn derive(
        sprites: &[Arc<ManagedImageResource>],
        max_texture_size: u32,
    ) -> Result<Self, LibraryError> {
        let padded = sprites
            .iter()
            .map(|sprite| {
                let image = sprite.image().pixels();
                if image.width() == 0 || image.height() == 0 {
                    return Err(LibraryError::Render(
                        "Sprite collection contains an empty image".into(),
                    ));
                }
                let width = image
                    .width()
                    .checked_add(ATLAS_GUTTER * 2)
                    .ok_or_else(|| LibraryError::Render("Sprite width overflow".into()))?;
                let height = image
                    .height()
                    .checked_add(ATLAS_GUTTER * 2)
                    .ok_or_else(|| LibraryError::Render("Sprite height overflow".into()))?;
                if width > max_texture_size || height > max_texture_size {
                    return Err(LibraryError::Render(format!(
                        "Sprite image {}x{} with filtering gutter exceeds GPU texture limit {max_texture_size}",
                        image.width(), image.height()
                    )));
                }
                Ok([width, height])
            })
            .collect::<Result<Vec<_>, _>>()?;
        let area = padded.iter().try_fold(0_u64, |area, size| {
            area.checked_add(u64::from(size[0]) * u64::from(size[1]))
                .ok_or_else(|| LibraryError::Render("Sprite atlas area overflow".into()))
        })?;
        let widest = padded.iter().map(|size| size[0]).max().unwrap_or(1);
        let square = (area as f64).sqrt().ceil().max(f64::from(widest)) as u32;
        let mut candidate = square
            .checked_next_power_of_two()
            .unwrap_or(max_texture_size)
            .min(max_texture_size)
            .max(widest);
        loop {
            if let Some((origins, height)) = shelf_pack(&padded, candidate, max_texture_size) {
                let entries = sprites
                    .iter()
                    .zip(&origins)
                    .map(|(sprite, origin)| {
                        let image = sprite.image().pixels();
                        let max_edge = image.width().max(image.height()) as f32;
                        SpriteAtlasEntry {
                            uv: [
                                (origin[0] + ATLAS_GUTTER) as f32 / candidate as f32,
                                (origin[1] + ATLAS_GUTTER) as f32 / height as f32,
                                (origin[0] + ATLAS_GUTTER + image.width()) as f32
                                    / candidate as f32,
                                (origin[1] + ATLAS_GUTTER + image.height()) as f32 / height as f32,
                            ],
                            aspect: [
                                image.width() as f32 / max_edge,
                                image.height() as f32 / max_edge,
                            ],
                        }
                    })
                    .collect();
                return Ok(Self {
                    width: candidate,
                    height,
                    origins,
                    entries,
                });
            }
            if candidate == max_texture_size {
                return Err(LibraryError::Render(format!(
                    "Sprite collection cannot fit within GPU texture limit {max_texture_size}"
                )));
            }
            candidate = candidate.saturating_mul(2).min(max_texture_size);
        }
    }

    fn pixels(&self, sprites: &[Arc<ManagedImageResource>]) -> Result<Vec<[f32; 4]>, LibraryError> {
        let count = u64::from(self.width)
            .checked_mul(u64::from(self.height))
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| LibraryError::Render("Sprite atlas allocation overflow".into()))?;
        let mut atlas = Vec::new();
        atlas.try_reserve_exact(count).map_err(|_| {
            LibraryError::Render(format!(
                "Cannot allocate {count} working pixels for the Sprite atlas"
            ))
        })?;
        atlas.resize(count, [0.0; 4]);
        for (sprite, origin) in sprites.iter().zip(&self.origins) {
            let image = sprite.image().pixels();
            let x0 = origin[0] + ATLAS_GUTTER;
            let y0 = origin[1] + ATLAS_GUTTER;
            for y in 0..image.height() {
                for x in 0..image.width() {
                    let source = image.pixels()[y as usize * image.width() as usize + x as usize];
                    set_pixel(&mut atlas, self.width, x0 + x, y0 + y, source);
                }
                let row = y as usize * image.width() as usize;
                let left = image.pixels()[row];
                let right = image.pixels()[row + image.width() as usize - 1];
                set_pixel(&mut atlas, self.width, x0 - 1, y0 + y, left);
                set_pixel(&mut atlas, self.width, x0 + image.width(), y0 + y, right);
            }
            for x in 0..image.width() {
                let top = image.pixels()[x as usize];
                let bottom = image.pixels()
                    [(image.height() as usize - 1) * image.width() as usize + x as usize];
                set_pixel(&mut atlas, self.width, x0 + x, y0 - 1, top);
                set_pixel(&mut atlas, self.width, x0 + x, y0 + image.height(), bottom);
            }
            let top_left = image.pixels()[0];
            let top_right = image.pixels()[(image.width() - 1) as usize];
            let bottom_left =
                image.pixels()[(image.height() as usize - 1) * image.width() as usize];
            let bottom_right = *image.pixels().last().ok_or_else(|| {
                LibraryError::Render("Sprite image lost its validated pixels".into())
            })?;
            set_pixel(&mut atlas, self.width, x0 - 1, y0 - 1, top_left);
            set_pixel(
                &mut atlas,
                self.width,
                x0 + image.width(),
                y0 - 1,
                top_right,
            );
            set_pixel(
                &mut atlas,
                self.width,
                x0 - 1,
                y0 + image.height(),
                bottom_left,
            );
            set_pixel(
                &mut atlas,
                self.width,
                x0 + image.width(),
                y0 + image.height(),
                bottom_right,
            );
        }
        Ok(atlas)
    }
}

fn shelf_pack(sizes: &[[u32; 2]], width: u32, max_height: u32) -> Option<(Vec<[u32; 2]>, u32)> {
    let mut origins = Vec::with_capacity(sizes.len());
    let mut x = 0_u32;
    let mut y = 0_u32;
    let mut row_height = 0_u32;
    for &[item_width, item_height] in sizes {
        if x.checked_add(item_width)? > width {
            y = y.checked_add(row_height)?;
            x = 0;
            row_height = 0;
        }
        if y.checked_add(item_height)? > max_height {
            return None;
        }
        origins.push([x, y]);
        x = x.checked_add(item_width)?;
        row_height = row_height.max(item_height);
    }
    let height = y.checked_add(row_height)?.max(1);
    Some((origins, height))
}

fn set_pixel(pixels: &mut [[f32; 4]], width: u32, x: u32, y: u32, value: [f32; 4]) {
    pixels[y as usize * width as usize + x as usize] = value;
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruvie_color_management::{
        BuiltinColorTransform, ColorContext, ColorTransformBackend, LINEAR_SRGB_SPACE_ID,
        LinearWorkingImage, ManagedLinearWorkingImage, WorkingColorIdentity,
    };

    fn resource(width: u32, height: u32, pixels: Vec<[f32; 4]>) -> Arc<ManagedImageResource> {
        let verified = BuiltinColorTransform
            .verify_working_space(LINEAR_SRGB_SPACE_ID, &ColorContext::default())
            .unwrap();
        let identity = WorkingColorIdentity::from_verified("sprite-atlas-unit", verified).unwrap();
        let pixels =
            LinearWorkingImage::from_premultiplied_rgba_f32(width, height, pixels).unwrap();
        // SAFETY: the fixture values are premultiplied linear-sRGB samples
        // matching the verified identity above.
        let image =
            unsafe { ManagedLinearWorkingImage::from_working_pixels_unchecked(identity, pixels) };
        Arc::new(ManagedImageResource::new(image))
    }

    #[test]
    fn shelf_pack_preserves_order_wraps_and_rejects_height_overflow() {
        assert_eq!(
            shelf_pack(&[[4, 3], [4, 3]], 8, 3),
            Some((vec![[0, 0], [4, 0]], 3))
        );
        assert_eq!(
            shelf_pack(&[[5, 2], [5, 3]], 6, 5),
            Some((vec![[0, 0], [0, 2]], 5))
        );
        assert_eq!(shelf_pack(&[[5, 2], [5, 3]], 6, 4), None);
    }

    #[test]
    fn atlas_layout_rejects_an_image_whose_gutter_exceeds_the_gpu_limit() {
        let image = resource(4, 4, vec![[1.0; 4]; 16]);
        let error = AtlasLayout::derive(&[image], 5)
            .err()
            .expect("oversized padded image must fail")
            .to_string();
        assert!(error.contains("with filtering gutter exceeds GPU texture limit 5"));
    }

    #[test]
    fn atlas_pixels_extrude_each_source_edge_and_corner() {
        let red = [1.0, 0.0, 0.0, 1.0];
        let green = [0.0, 1.0, 0.0, 1.0];
        let blue = [0.0, 0.0, 1.0, 1.0];
        let white = [1.0; 4];
        let image = resource(2, 2, vec![red, green, blue, white]);
        let layout = AtlasLayout::derive(std::slice::from_ref(&image), 64).unwrap();
        let pixels = layout.pixels(&[image]).unwrap();
        let origin = layout.origins[0];
        let at = |x: u32, y: u32| pixels[y as usize * layout.width as usize + x as usize];
        let x = origin[0] + ATLAS_GUTTER;
        let y = origin[1] + ATLAS_GUTTER;

        assert_eq!(
            [at(x, y), at(x + 1, y), at(x, y + 1), at(x + 1, y + 1)],
            [red, green, blue, white]
        );
        assert_eq!([at(x - 1, y), at(x - 1, y + 1)], [red, blue]);
        assert_eq!([at(x + 2, y), at(x + 2, y + 1)], [green, white]);
        assert_eq!([at(x, y - 1), at(x + 1, y - 1)], [red, green]);
        assert_eq!([at(x, y + 2), at(x + 1, y + 2)], [blue, white]);
        assert_eq!(
            [
                at(x - 1, y - 1),
                at(x + 2, y - 1),
                at(x - 1, y + 2),
                at(x + 2, y + 2)
            ],
            [red, green, blue, white]
        );
    }
}
