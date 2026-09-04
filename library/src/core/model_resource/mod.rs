//! Shared, format-neutral model-resource decoding and caching.

mod fbx;
mod model;
mod service;

pub use model::{
    EmbeddedModelTexture, MeshMaterial, MeshPrimitive, MeshScene, MeshSceneNode, MeshVertex,
    ModelCoordinateSpace, ModelDecoderIdentity, ModelDiagnostic, ModelDiagnosticCode,
    ModelDiagnosticSeverity, ModelNormalizationSettings, ModelResourceKey, ModelSourceFormat,
    ModelSourceMetadata, StaticTriangleMesh,
};
pub use service::{ModelDecodeLimits, ModelResourceError, ModelResourceService};

#[cfg(test)]
mod tests;
