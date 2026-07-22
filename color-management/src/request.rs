use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

/// Why a color transform is requested.
///
/// This describes the pipeline boundary. It does not alter the mathematical
/// transform, which is represented independently by [`TransformSpec`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransformPurpose {
    Explicit,
    SourceToWorking,
    WorkingToDisplay,
    WorkingToOutput,
}

/// The exact mathematical transform a backend must create.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TransformSpec {
    ColorSpace {
        source: String,
        destination: String,
    },
    DisplayView {
        source: String,
        display: String,
        view: String,
        looks_bypass: bool,
        data_bypass: bool,
    },
}

/// Immutable OpenColorIO-style context variables and their stable identity.
///
/// A sorted map makes equivalent contexts independent of insertion order. The
/// fingerprint uses length-framed UTF-8 fields, so neither separators nor
/// ambiguous concatenation can change the identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ColorContext {
    variables: BTreeMap<String, String>,
    fingerprint: String,
}

impl ColorContext {
    pub fn new(variables: BTreeMap<String, String>) -> Self {
        let fingerprint = fingerprint_variables(&variables);
        Self {
            variables,
            fingerprint,
        }
    }

    pub fn from_variables(
        variables: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        Self::new(
            variables
                .into_iter()
                .map(|(name, value)| (name.into(), value.into()))
                .collect(),
        )
    }

    #[must_use]
    pub fn with_variable(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.variables.insert(name.into(), value.into());
        self.fingerprint = fingerprint_variables(&self.variables);
        self
    }

    pub fn variables(&self) -> &BTreeMap<String, String> {
        &self.variables
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

impl Default for ColorContext {
    fn default() -> Self {
        Self::new(BTreeMap::new())
    }
}

/// Complete immutable request used to compile a color processor.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ColorTransformRequest {
    purpose: TransformPurpose,
    spec: TransformSpec,
    context: ColorContext,
}

impl ColorTransformRequest {
    pub fn explicit(source: impl Into<String>, destination: impl Into<String>) -> Self {
        Self::color_space(TransformPurpose::Explicit, source, destination)
    }

    pub fn source_to_working(source: impl Into<String>, working_space: impl Into<String>) -> Self {
        Self::color_space(TransformPurpose::SourceToWorking, source, working_space)
    }

    /// Direct color-space conversion for a display boundary without a named
    /// display/view pair. This is the built-in backend's limited preview path.
    pub fn working_to_display(
        working_space: impl Into<String>,
        display_space: impl Into<String>,
    ) -> Self {
        Self::color_space(
            TransformPurpose::WorkingToDisplay,
            working_space,
            display_space,
        )
    }

    /// Named display/view transform, as defined by an OpenColorIO config.
    pub fn working_to_display_view(
        working_space: impl Into<String>,
        display: impl Into<String>,
        view: impl Into<String>,
    ) -> Self {
        Self::working_to_display_view_with_options(working_space, display, view, false, false)
    }

    pub fn working_to_display_view_with_options(
        working_space: impl Into<String>,
        display: impl Into<String>,
        view: impl Into<String>,
        looks_bypass: bool,
        data_bypass: bool,
    ) -> Self {
        Self {
            purpose: TransformPurpose::WorkingToDisplay,
            spec: TransformSpec::DisplayView {
                source: working_space.into(),
                display: display.into(),
                view: view.into(),
                looks_bypass,
                data_bypass,
            },
            context: ColorContext::default(),
        }
    }

    pub fn working_to_output(
        working_space: impl Into<String>,
        output_space: impl Into<String>,
    ) -> Self {
        Self::color_space(
            TransformPurpose::WorkingToOutput,
            working_space,
            output_space,
        )
    }

    #[must_use]
    pub fn with_context(mut self, context: ColorContext) -> Self {
        self.context = context;
        self
    }

    pub const fn purpose(&self) -> TransformPurpose {
        self.purpose
    }

    pub const fn spec(&self) -> &TransformSpec {
        &self.spec
    }

    pub const fn context(&self) -> &ColorContext {
        &self.context
    }

    fn color_space(
        purpose: TransformPurpose,
        source: impl Into<String>,
        destination: impl Into<String>,
    ) -> Self {
        Self {
            purpose,
            spec: TransformSpec::ColorSpace {
                source: source.into(),
                destination: destination.into(),
            },
            context: ColorContext::default(),
        }
    }
}

/// Collision-resistant structural identity for reusing a compiled processor.
///
/// The full typed spec and context participate in equality and hashing. The
/// context fingerprint remains available to native backends, but is not used
/// as a lossy substitute for the variables themselves.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProcessorCacheKey {
    pub backend_id: String,
    pub config_fingerprint: String,
    pub purpose: TransformPurpose,
    pub spec: TransformSpec,
    pub context: ColorContext,
}

fn fingerprint_variables(variables: &BTreeMap<String, String>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"ruvie-color-context-v1\0");
    for (name, value) in variables {
        update_length_framed(&mut hasher, name.as_bytes());
        update_length_framed(&mut hasher, value.as_bytes());
    }
    encode_hex(&hasher.finalize())
}

fn update_length_framed(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(value.len().to_string().as_bytes());
    hasher.update(b":");
    hasher.update(value);
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{
        ColorContext, ColorTransformRequest, ProcessorCacheKey, TransformPurpose, TransformSpec,
    };

    #[test]
    fn context_identity_is_stable_across_insertion_order_and_unambiguous() {
        let first = ColorContext::from_variables([("SHOT", "010"), ("SEQ", "A")]);
        let reordered = ColorContext::from_variables([("SEQ", "A"), ("SHOT", "010")]);
        let ambiguous_without_framing = ColorContext::from_variables([("SH", "OT010SEQ=A")]);

        assert_eq!(first, reordered);
        assert_eq!(first.fingerprint(), reordered.fingerprint());
        assert_ne!(first.fingerprint(), ambiguous_without_framing.fingerprint());
    }

    #[test]
    fn cache_key_structurally_separates_purpose_spec_options_and_context() {
        let config = "config-sha".to_string();
        let make_key = |request: ColorTransformRequest| ProcessorCacheKey {
            backend_id: "test".to_string(),
            config_fingerprint: config.clone(),
            purpose: request.purpose(),
            spec: request.spec().clone(),
            context: request.context().clone(),
        };
        let direct = ColorTransformRequest::working_to_display("acescg", "srgb");
        let view = ColorTransformRequest::working_to_display_view("acescg", "sRGB", "Film");
        let bypassed = ColorTransformRequest::working_to_display_view_with_options(
            "acescg", "sRGB", "Film", true, false,
        );
        let contextual = direct
            .clone()
            .with_context(ColorContext::default().with_variable("SHOT", "010"));
        let explicit = ColorTransformRequest::explicit("acescg", "srgb");

        assert_eq!(direct.purpose(), TransformPurpose::WorkingToDisplay);
        assert!(matches!(direct.spec(), TransformSpec::ColorSpace { .. }));
        assert_ne!(make_key(direct.clone()), make_key(view));
        assert_ne!(make_key(direct.clone()), make_key(bypassed));
        assert_ne!(make_key(direct.clone()), make_key(contextual));
        assert_ne!(make_key(direct), make_key(explicit));
    }
}
