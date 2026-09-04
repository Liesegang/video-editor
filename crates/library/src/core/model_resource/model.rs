//! Format-neutral, fully owned model resources.
//!
//! These values are derived runtime data. They intentionally do not implement
//! `Serialize` or `Deserialize`: the Project stores an Asset reference and an
//! import fingerprint, never parser output or GPU-ready mesh buffers.

use std::mem::size_of;

/// Cache identity for a decoded model resource.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ModelResourceKey {
    pub source_sha256: [u8; 32],
    pub decoder: ModelDecoderIdentity,
    pub normalization: ModelNormalizationSettings,
    pub supported_feature_version: u32,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct ModelDecoderIdentity {
    pub implementation: &'static str,
    pub version: &'static str,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct ModelNormalizationSettings {
    pub target_space: ModelCoordinateSpace,
    pub generate_missing_normals: bool,
}

impl Default for ModelNormalizationSettings {
    fn default() -> Self {
        Self {
            target_space: ModelCoordinateSpace::RightHandedYUpMeters,
            generate_missing_normals: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum ModelCoordinateSpace {
    RightHandedYUpMeters,
}

/// A format-neutral scene whose buffers and strings own all of their data.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshScene {
    pub key: ModelResourceKey,
    pub nodes: Vec<MeshSceneNode>,
    pub meshes: Vec<StaticTriangleMesh>,
    pub materials: Vec<MeshMaterial>,
    pub textures: Vec<EmbeddedModelTexture>,
    pub diagnostics: Vec<ModelDiagnostic>,
    pub source_metadata: ModelSourceMetadata,
}

impl MeshScene {
    /// Approximate resident size used by the shared rebuildable LRU.
    ///
    /// Vector allocations and owned payloads dominate model resources. The
    /// value deliberately includes every owned string and encoded texture but
    /// excludes allocator bookkeeping and the cache key itself.
    pub fn estimated_resident_bytes(&self) -> usize {
        let node_bytes = self
            .nodes
            .capacity()
            .saturating_mul(size_of::<MeshSceneNode>())
            .saturating_add(
                self.nodes
                    .iter()
                    .map(|node| {
                        node.name.capacity().saturating_add(
                            node.material_slots
                                .capacity()
                                .saturating_mul(size_of::<usize>()),
                        )
                    })
                    .sum::<usize>(),
            );
        let mesh_bytes = self
            .meshes
            .iter()
            .map(|mesh| {
                mesh.name
                    .capacity()
                    .saturating_add(
                        mesh.vertices
                            .capacity()
                            .saturating_mul(size_of::<MeshVertex>()),
                    )
                    .saturating_add(mesh.indices.capacity().saturating_mul(size_of::<u32>()))
                    .saturating_add(
                        mesh.primitives
                            .capacity()
                            .saturating_mul(size_of::<MeshPrimitive>()),
                    )
            })
            .sum::<usize>()
            .saturating_add(
                self.meshes
                    .capacity()
                    .saturating_mul(size_of::<StaticTriangleMesh>()),
            );
        let material_bytes = self
            .materials
            .iter()
            .map(|material| material.name.capacity())
            .sum::<usize>()
            .saturating_add(
                self.materials
                    .capacity()
                    .saturating_mul(size_of::<MeshMaterial>()),
            );
        let texture_bytes = self
            .textures
            .iter()
            .map(|texture| {
                texture
                    .name
                    .capacity()
                    .saturating_add(texture.encoded_bytes.capacity())
            })
            .sum::<usize>()
            .saturating_add(
                self.textures
                    .capacity()
                    .saturating_mul(size_of::<EmbeddedModelTexture>()),
            );
        let diagnostic_bytes = self
            .diagnostics
            .iter()
            .map(|diagnostic| {
                diagnostic.message.capacity().saturating_add(
                    diagnostic
                        .element_name
                        .as_ref()
                        .map_or(0, std::string::String::capacity),
                )
            })
            .sum::<usize>()
            .saturating_add(
                self.diagnostics
                    .capacity()
                    .saturating_mul(size_of::<ModelDiagnostic>()),
            );
        size_of::<Self>()
            .saturating_add(node_bytes)
            .saturating_add(mesh_bytes)
            .saturating_add(material_bytes)
            .saturating_add(texture_bytes)
            .saturating_add(diagnostic_bytes)
            .saturating_add(self.source_metadata.creator.capacity())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeshSceneNode {
    pub name: String,
    pub parent: Option<usize>,
    pub mesh: Option<usize>,
    /// Global material indices for the mesh's local material slots.
    ///
    /// FBX permits the same geometry to be instanced with different materials,
    /// so material ownership belongs to the node invocation rather than the
    /// shared mesh primitive.
    pub material_slots: Vec<usize>,
    /// Column-major affine matrix in normalized scene coordinates.
    pub local_transform: [[f32; 4]; 4],
    /// Column-major affine matrix in normalized scene coordinates.
    pub world_transform: [[f32; 4]; 4],
    pub visible: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StaticTriangleMesh {
    pub name: String,
    /// Source polygon-face count retained for strict cache-hit limit checks.
    pub source_face_count: usize,
    /// One owned vertex per FBX polygon vertex. This keeps position, normal,
    /// and UV seams exact while triangle indices remain compact and checked.
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
    pub primitives: Vec<MeshPrimitive>,
    pub has_source_normals: bool,
    pub has_uv0: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv0: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshPrimitive {
    pub first_index: u32,
    pub index_count: u32,
    /// Local slot resolved through `MeshSceneNode::material_slots`.
    pub material_slot: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeshMaterial {
    pub name: String,
    pub base_color: [f32; 4],
    pub base_color_texture: Option<usize>,
}

/// Encoded embedded texture data copied out of parser-owned memory.
/// Decoding pixels is intentionally left to the shared image-resource path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddedModelTexture {
    pub name: String,
    pub encoded_bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelSourceMetadata {
    pub format: ModelSourceFormat,
    pub source_version: u32,
    pub creator: String,
    pub original_unit_meters: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelSourceFormat {
    Fbx,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelDiagnostic {
    pub severity: ModelDiagnosticSeverity,
    pub code: ModelDiagnosticCode,
    pub message: String,
    pub element_name: Option<String>,
}

impl ModelDiagnostic {
    pub(crate) fn warning(
        code: ModelDiagnosticCode,
        message: impl Into<String>,
        element_name: Option<String>,
    ) -> Self {
        Self {
            severity: ModelDiagnosticSeverity::Warning,
            code,
            message: message.into(),
            element_name,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelDiagnosticSeverity {
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelDiagnosticCode {
    ParserWarning,
    AnimationUnsupported,
    SkinningUnsupported,
    MorphTargetsUnsupported,
    GeometryCacheUnsupported,
    AdditionalUvSetsUnsupported,
    AdvancedMaterialUnsupported,
    ExternalTextureNotLoaded,
    TextureUvTransformUnsupported,
    TextureUvSetSelectionUnsupported,
    TextureWrapModeUnsupported,
    LayeredTextureUnsupported,
    ProceduralTextureUnsupported,
    ShaderTextureUnsupported,
    CameraUnsupported,
    LightUnsupported,
    UnsupportedGeometry,
    DegenerateNormalGenerated,
}
