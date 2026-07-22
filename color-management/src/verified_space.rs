use crate::{BackendBuild, ColorContext};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorReferenceSpace {
    Scene,
    Display,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorLinearity {
    Linear,
    Encoded,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ColorSpaceInfo {
    pub id: String,
    pub label: String,
    pub reference_space: ColorReferenceSpace,
    pub linearity: ColorLinearity,
    pub is_data: bool,
}

impl ColorSpaceInfo {
    pub const fn is_valid_working_space(&self) -> bool {
        matches!(self.reference_space, ColorReferenceSpace::Scene)
            && matches!(self.linearity, ColorLinearity::Linear)
            && !self.is_data
    }
}

/// Identity shared by backend-issued color-space capabilities.
///
/// This type and all constructors are crate-private. Public tokens expose only
/// read access, so an application cannot mint a token from a space name or a
/// self-reported backend identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct VerifiedBackendContext {
    backend_id: String,
    backend_build: BackendBuild,
    backend_config_fingerprint: String,
    context: ColorContext,
}

impl VerifiedBackendContext {
    fn new(
        backend_id: String,
        backend_build: BackendBuild,
        backend_config_fingerprint: String,
        context: ColorContext,
    ) -> Self {
        Self {
            backend_id,
            backend_build,
            backend_config_fingerprint,
            context,
        }
    }
}

/// A resolved, non-data source color space issued by one trusted backend.
///
/// Unlike a source-space string, this token is bound to the exact backend
/// configuration and context used to compile a source-to-working processor.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct VerifiedSourceSpace {
    backend: VerifiedBackendContext,
    color_space: ColorSpaceInfo,
}

impl VerifiedSourceSpace {
    pub(crate) fn new(
        backend_id: String,
        backend_build: BackendBuild,
        backend_config_fingerprint: String,
        context: ColorContext,
        color_space: ColorSpaceInfo,
    ) -> Self {
        Self {
            backend: VerifiedBackendContext::new(
                backend_id,
                backend_build,
                backend_config_fingerprint,
                context,
            ),
            color_space,
        }
    }

    pub fn backend_id(&self) -> &str {
        &self.backend.backend_id
    }

    pub const fn backend_build(&self) -> BackendBuild {
        self.backend.backend_build
    }

    pub fn backend_config_fingerprint(&self) -> &str {
        &self.backend.backend_config_fingerprint
    }

    pub const fn context(&self) -> &ColorContext {
        &self.backend.context
    }

    pub fn color_space_id(&self) -> &str {
        &self.color_space.id
    }

    pub const fn color_space(&self) -> &ColorSpaceInfo {
        &self.color_space
    }
}

/// A scene-linear, non-data working space issued by one trusted backend.
///
/// The token captures the exact backend configuration and context that every
/// processor crossing the working-image boundary must use.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct VerifiedWorkingSpace {
    backend: VerifiedBackendContext,
    color_space: ColorSpaceInfo,
}

impl VerifiedWorkingSpace {
    pub(crate) fn new(
        backend_id: String,
        backend_build: BackendBuild,
        backend_config_fingerprint: String,
        context: ColorContext,
        color_space: ColorSpaceInfo,
    ) -> Self {
        Self {
            backend: VerifiedBackendContext::new(
                backend_id,
                backend_build,
                backend_config_fingerprint,
                context,
            ),
            color_space,
        }
    }

    pub fn backend_id(&self) -> &str {
        &self.backend.backend_id
    }

    pub const fn backend_build(&self) -> BackendBuild {
        self.backend.backend_build
    }

    pub fn backend_config_fingerprint(&self) -> &str {
        &self.backend.backend_config_fingerprint
    }

    pub const fn context(&self) -> &ColorContext {
        &self.backend.context
    }

    pub fn color_space_id(&self) -> &str {
        &self.color_space.id
    }

    pub const fn color_space(&self) -> &ColorSpaceInfo {
        &self.color_space
    }

    pub(crate) fn has_same_backend_context(&self, source: &VerifiedSourceSpace) -> bool {
        self.backend == source.backend
    }
}
