use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use thiserror::Error;

use super::fbx::decode_fbx;
use super::{
    MeshScene, ModelDecoderIdentity, ModelDiagnostic, ModelNormalizationSettings, ModelResourceKey,
};
use crate::core::cache::SharedCacheManager;
use crate::model::asset::{Asset, AssetKind};
use crate::util::local_file::DirectRegularFile;

pub(crate) const UFBX_DECODER: ModelDecoderIdentity = ModelDecoderIdentity {
    implementation: "ufbx",
    version: "0.11.3",
};
pub(crate) const SUPPORTED_FEATURE_VERSION: u32 = 1;

/// Hard limits applied before parser or owned-scene allocations are exposed.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ModelDecodeLimits {
    pub max_source_bytes: usize,
    pub max_parser_bytes: usize,
    /// Cumulative Rust-side scratch storage requested while copying a scene.
    pub max_working_bytes: usize,
    pub max_scene_bytes: usize,
    pub max_nodes: usize,
    pub max_hierarchy_depth: usize,
    pub max_meshes: usize,
    pub max_faces: usize,
    pub max_vertices: usize,
    pub max_indices: usize,
    pub max_materials: usize,
    pub max_textures: usize,
    pub max_embedded_texture_bytes: usize,
}

impl Default for ModelDecodeLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 128 * 1024 * 1024,
            max_parser_bytes: 512 * 1024 * 1024,
            max_working_bytes: 512 * 1024 * 1024,
            max_scene_bytes: 512 * 1024 * 1024,
            max_nodes: 100_000,
            max_hierarchy_depth: 512,
            max_meshes: 10_000,
            max_faces: 10_000_000,
            max_vertices: 10_000_000,
            max_indices: 30_000_000,
            max_materials: 16_384,
            max_textures: 4_096,
            max_embedded_texture_bytes: 128 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Error)]
pub enum ModelResourceError {
    #[error("Asset {asset_name:?} is {actual:?}, not a 3D model")]
    UnsupportedAssetKind {
        asset_name: String,
        actual: AssetKind,
    },
    #[error("unsupported model format for {path:?}; the first decoder slice accepts FBX only")]
    UnsupportedFormat { path: PathBuf },
    #[error("Model Asset {asset_name:?} has no imported-content SHA-256")]
    MissingImportedFingerprint { asset_name: String },
    #[error("Model source fingerprint changed: expected {expected}, got {actual}")]
    FingerprintMismatch { expected: String, actual: String },
    #[error("invalid model decode limits: {detail}")]
    InvalidLimits { detail: String },
    #[error("model {resource} budget exceeded: {actual} > {limit}")]
    BudgetExceeded {
        resource: &'static str,
        actual: usize,
        limit: usize,
    },
    #[error("cannot allocate {requested} bytes for model {resource}")]
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    #[error("cannot read model source {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: Arc<std::io::Error>,
    },
    #[error("FBX decode failed: {detail}")]
    Decode { detail: String },
    #[error("invalid decoded model data: {detail}")]
    InvalidData { detail: String },
    #[error("model contains no supported renderable triangle geometry")]
    NoRenderableGeometry { diagnostics: Vec<ModelDiagnostic> },
}

/// The sole model decoding boundary shared by editor and render consumers.
///
/// Clones keep the same application `CacheManager`; decoded scenes therefore
/// remain one rebuildable derived resource rather than per-panel parser state.
#[derive(Clone)]
pub struct ModelResourceService {
    cache: SharedCacheManager,
    limits: ModelDecodeLimits,
    normalization: ModelNormalizationSettings,
}

impl ModelResourceService {
    pub fn new(cache: SharedCacheManager) -> Self {
        Self {
            cache,
            limits: ModelDecodeLimits::default(),
            normalization: ModelNormalizationSettings::default(),
        }
    }

    pub fn with_limits(
        cache: SharedCacheManager,
        limits: ModelDecodeLimits,
    ) -> Result<Self, ModelResourceError> {
        Self::with_configuration(cache, limits, ModelNormalizationSettings::default())
    }

    pub fn with_configuration(
        cache: SharedCacheManager,
        limits: ModelDecodeLimits,
        normalization: ModelNormalizationSettings,
    ) -> Result<Self, ModelResourceError> {
        validate_limits(&limits)?;
        Ok(Self {
            cache,
            limits,
            normalization,
        })
    }

    /// Decodes FBX bytes without retaining a parser scene or borrowing input.
    pub fn decode_fbx_bytes(&self, bytes: &[u8]) -> Result<Arc<MeshScene>, ModelResourceError> {
        self.decode_checked_bytes(bytes)
    }

    /// Opens a direct local regular file and decodes it as FBX.
    pub fn load_fbx_file(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<Arc<MeshScene>, ModelResourceError> {
        let path = path.as_ref();
        require_fbx_extension(path)?;
        let bytes = read_bounded_file(path, self.limits.max_source_bytes)?;
        self.decode_checked_bytes(&bytes)
    }

    /// Resolves a Project Asset while enforcing its import-time fingerprint.
    pub fn load_asset(&self, asset: &Asset) -> Result<Arc<MeshScene>, ModelResourceError> {
        if asset.kind != AssetKind::Model3D {
            return Err(ModelResourceError::UnsupportedAssetKind {
                asset_name: asset.name.clone(),
                actual: asset.kind.clone(),
            });
        }
        let path = Path::new(&asset.path);
        require_fbx_extension(path)?;
        let expected = asset.imported_content_sha256().ok_or_else(|| {
            ModelResourceError::MissingImportedFingerprint {
                asset_name: asset.name.clone(),
            }
        })?;
        let bytes = read_bounded_file(path, self.limits.max_source_bytes)?;
        let actual = hex_sha256(&bytes);
        if !expected.eq_ignore_ascii_case(&actual) {
            return Err(ModelResourceError::FingerprintMismatch {
                expected: expected.to_string(),
                actual,
            });
        }
        self.decode_checked_bytes(&bytes)
    }

    fn decode_checked_bytes(&self, bytes: &[u8]) -> Result<Arc<MeshScene>, ModelResourceError> {
        enforce_limit("source bytes", bytes.len(), self.limits.max_source_bytes)?;
        let key = ModelResourceKey {
            source_sha256: Sha256::digest(bytes).into(),
            decoder: UFBX_DECODER,
            normalization: self.normalization,
            supported_feature_version: SUPPORTED_FEATURE_VERSION,
        };
        if let Some(scene) = self.cache.get_model_scene(&key) {
            validate_owned_scene_limits(&scene, &self.limits)?;
            return Ok(scene);
        }
        let decode_key = key.clone();
        let decode_limits = self.limits.clone();
        let scene = self
            .cache
            .get_or_decode_model_scene(key, self.limits.clone(), || {
                let scene = Arc::new(decode_fbx(bytes, decode_key, &decode_limits)?);
                enforce_limit(
                    "owned scene bytes",
                    scene.estimated_resident_bytes(),
                    decode_limits.max_scene_bytes,
                )?;
                Ok(scene)
            })?;
        // A differently configured service may have populated the semantic
        // cache while this request was joining its limits-specific flight.
        validate_owned_scene_limits(&scene, &self.limits)?;
        Ok(scene)
    }
}

pub(crate) fn enforce_limit(
    resource: &'static str,
    actual: usize,
    limit: usize,
) -> Result<(), ModelResourceError> {
    if actual > limit {
        return Err(ModelResourceError::BudgetExceeded {
            resource,
            actual,
            limit,
        });
    }
    Ok(())
}

fn validate_limits(limits: &ModelDecodeLimits) -> Result<(), ModelResourceError> {
    let values = [
        ("source bytes", limits.max_source_bytes),
        ("parser bytes", limits.max_parser_bytes),
        ("decode working bytes", limits.max_working_bytes),
        ("scene bytes", limits.max_scene_bytes),
        ("nodes", limits.max_nodes),
        ("hierarchy depth", limits.max_hierarchy_depth),
        ("meshes", limits.max_meshes),
        ("faces", limits.max_faces),
        ("vertices", limits.max_vertices),
        ("indices", limits.max_indices),
        ("materials", limits.max_materials),
        ("textures", limits.max_textures),
        ("embedded texture bytes", limits.max_embedded_texture_bytes),
    ];
    if let Some((name, _)) = values.into_iter().find(|(_, value)| *value == 0) {
        return Err(ModelResourceError::InvalidLimits {
            detail: format!("{name} must be greater than zero"),
        });
    }
    if limits.max_hierarchy_depth > u32::MAX as usize {
        return Err(ModelResourceError::InvalidLimits {
            detail: "hierarchy depth does not fit the FBX decoder contract".to_string(),
        });
    }
    if limits.max_parser_bytes < 3 {
        return Err(ModelResourceError::InvalidLimits {
            detail: "parser bytes must be at least three so all parser arenas stay bounded"
                .to_string(),
        });
    }
    Ok(())
}

fn validate_owned_scene_limits(
    scene: &MeshScene,
    limits: &ModelDecodeLimits,
) -> Result<(), ModelResourceError> {
    enforce_limit("nodes", scene.nodes.len(), limits.max_nodes)?;
    enforce_limit("meshes", scene.meshes.len(), limits.max_meshes)?;
    enforce_limit("materials", scene.materials.len(), limits.max_materials)?;
    enforce_limit("textures", scene.textures.len(), limits.max_textures)?;
    let vertices = scene
        .meshes
        .iter()
        .fold(0_usize, |sum, mesh| sum.saturating_add(mesh.vertices.len()));
    let indices = scene
        .meshes
        .iter()
        .fold(0_usize, |sum, mesh| sum.saturating_add(mesh.indices.len()));
    let faces = scene.meshes.iter().fold(0_usize, |sum, mesh| {
        sum.saturating_add(mesh.source_face_count)
    });
    let texture_bytes = scene.textures.iter().fold(0_usize, |sum, texture| {
        sum.saturating_add(texture.encoded_bytes.len())
    });
    enforce_limit("vertices", vertices, limits.max_vertices)?;
    enforce_limit("indices", indices, limits.max_indices)?;
    enforce_limit("faces", faces, limits.max_faces)?;
    enforce_limit(
        "embedded texture bytes",
        texture_bytes,
        limits.max_embedded_texture_bytes,
    )?;
    enforce_limit(
        "owned scene bytes",
        scene.estimated_resident_bytes(),
        limits.max_scene_bytes,
    )?;
    for start in 0..scene.nodes.len() {
        let mut depth = 0_usize;
        let mut cursor = Some(start);
        while let Some(index) = cursor {
            depth = depth.saturating_add(1);
            enforce_limit(
                "hierarchy depth",
                depth.saturating_sub(1),
                limits.max_hierarchy_depth,
            )?;
            cursor = scene.nodes[index].parent;
        }
    }
    Ok(())
}

fn require_fbx_extension(path: &Path) -> Result<(), ModelResourceError> {
    let is_fbx = path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("fbx"));
    if !is_fbx {
        return Err(ModelResourceError::UnsupportedFormat {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

fn read_bounded_file(path: &Path, max_bytes: usize) -> Result<Vec<u8>, ModelResourceError> {
    let opened = DirectRegularFile::open(path).map_err(|source| ModelResourceError::Io {
        path: path.to_path_buf(),
        source: Arc::new(source),
    })?;
    let reported_size = opened
        .file()
        .metadata()
        .map_err(|source| ModelResourceError::Io {
            path: path.to_path_buf(),
            source: Arc::new(source),
        })?
        .len();
    if reported_size > max_bytes as u64 {
        return Err(ModelResourceError::BudgetExceeded {
            resource: "source bytes",
            actual: usize::try_from(reported_size).unwrap_or(usize::MAX),
            limit: max_bytes,
        });
    }
    let read_limit = u64::try_from(max_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(reported_size as usize)
        .map_err(|_| ModelResourceError::AllocationFailed {
            resource: "source bytes",
            requested: reported_size as usize,
        })?;
    opened
        .into_file()
        .take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|source| ModelResourceError::Io {
            path: path.to_path_buf(),
            source: Arc::new(source),
        })?;
    enforce_limit("source bytes", bytes.len(), max_bytes)?;
    Ok(bytes)
}

fn hex_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
