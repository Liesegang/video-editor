//! Verified H.273 matrix/range contract for FFmpeg planar YUV decoding.

use crate::model::asset::{SourceColorRange, SourceMatrixCoefficients};
use crate::plugin::{AppliedYuvChromaLocation, YuvChromaLocation, YuvChromaLocationSource};
use ffmpeg_next as ffmpeg;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct H273YuvToRgb {
    matrix: H273Matrix,
    range: H273Range,
    chroma_location: ChromaLocationResolution,
}

impl H273YuvToRgb {
    pub(super) const fn new(
        matrix: H273Matrix,
        range: H273Range,
        chroma_location: ChromaLocationResolution,
    ) -> Self {
        Self {
            matrix,
            range,
            chroma_location,
        }
    }

    pub(super) const fn matrix(self) -> H273Matrix {
        self.matrix
    }

    pub(super) const fn range(self) -> H273Range {
        self.range
    }

    pub(super) const fn applied_chroma_location(self) -> Option<AppliedYuvChromaLocation> {
        self.chroma_location.applied()
    }

    pub(super) fn chroma_location(
        self,
        frame_location: ffmpeg::util::chroma::Location,
    ) -> Result<YuvChromaLocation, &'static str> {
        self.chroma_location.for_frame(frame_location)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChromaLocationResolution {
    /// Test/adapter plans created without a decoder resolve the current frame
    /// metadata at the pixel boundary.
    #[cfg(test)]
    DeferredFrame,
    Resolved(AppliedYuvChromaLocation),
    UnspecifiedFrameAndDecoder,
}

impl ChromaLocationResolution {
    #[cfg(test)]
    pub(super) const fn deferred_frame() -> Self {
        Self::DeferredFrame
    }

    pub(super) fn from_frame_and_decoder(
        frame: ffmpeg::util::chroma::Location,
        decoder: ffmpeg::util::chroma::Location,
    ) -> Self {
        if let Some(location) = map_chroma_location(frame) {
            return Self::Resolved(AppliedYuvChromaLocation::new(
                location,
                YuvChromaLocationSource::Frame,
            ));
        }
        if let Some(location) = map_chroma_location(decoder) {
            return Self::Resolved(AppliedYuvChromaLocation::new(
                location,
                YuvChromaLocationSource::Decoder,
            ));
        }
        Self::UnspecifiedFrameAndDecoder
    }

    const fn applied(self) -> Option<AppliedYuvChromaLocation> {
        match self {
            Self::Resolved(location) => Some(location),
            #[cfg(test)]
            Self::DeferredFrame => None,
            Self::UnspecifiedFrameAndDecoder => None,
        }
    }

    fn for_frame(
        self,
        frame_location: ffmpeg::util::chroma::Location,
    ) -> Result<YuvChromaLocation, &'static str> {
        match self {
            Self::Resolved(location) => {
                let current = map_chroma_location(frame_location);
                match location.source() {
                    YuvChromaLocationSource::Frame if current == Some(location.location()) => {
                        Ok(location.location())
                    }
                    YuvChromaLocationSource::Frame => {
                        Err("current frame chroma location differs from the verified frame plan")
                    }
                    YuvChromaLocationSource::Decoder
                        if current.is_none() || current == Some(location.location()) =>
                    {
                        Ok(location.location())
                    }
                    YuvChromaLocationSource::Decoder => Err(
                        "current frame chroma location contradicts the verified decoder fallback",
                    ),
                }
            }
            #[cfg(test)]
            Self::DeferredFrame => map_chroma_location(frame_location)
                .ok_or("subsampled YUV frame has unspecified chroma location"),
            Self::UnspecifiedFrameAndDecoder => {
                Err("subsampled YUV frame and decoder both have unspecified chroma location")
            }
        }
    }
}

fn map_chroma_location(value: ffmpeg::util::chroma::Location) -> Option<YuvChromaLocation> {
    use ffmpeg::util::chroma::Location;
    match value {
        Location::Left => Some(YuvChromaLocation::Left),
        Location::Center => Some(YuvChromaLocation::Center),
        Location::TopLeft => Some(YuvChromaLocation::TopLeft),
        Location::Top => Some(YuvChromaLocation::Top),
        Location::BottomLeft => Some(YuvChromaLocation::BottomLeft),
        Location::Bottom => Some(YuvChromaLocation::Bottom),
        Location::Unspecified => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct H273Matrix {
    kr: f32,
    kb: f32,
}

impl H273Matrix {
    pub(super) const fn kr(self) -> f32 {
        self.kr
    }

    pub(super) const fn kb(self) -> f32 {
        self.kb
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum H273Range {
    Limited,
    Full,
}

pub(super) fn matrix(
    value: ffmpeg::color::Space,
) -> Option<(H273Matrix, SourceMatrixCoefficients)> {
    use ffmpeg::color::Space as Ffmpeg;
    let mapped = match value {
        Ffmpeg::BT709 => (
            H273Matrix {
                kr: 0.2126,
                kb: 0.0722,
            },
            SourceMatrixCoefficients::Bt709,
        ),
        Ffmpeg::FCC => (
            H273Matrix { kr: 0.30, kb: 0.11 },
            SourceMatrixCoefficients::Fcc,
        ),
        Ffmpeg::BT470BG => (
            H273Matrix {
                kr: 0.299,
                kb: 0.114,
            },
            SourceMatrixCoefficients::Bt470Bg,
        ),
        Ffmpeg::SMPTE170M => (
            H273Matrix {
                kr: 0.299,
                kb: 0.114,
            },
            SourceMatrixCoefficients::Smpte170M,
        ),
        Ffmpeg::SMPTE240M => (
            H273Matrix {
                kr: 0.212,
                kb: 0.087,
            },
            SourceMatrixCoefficients::Smpte240M,
        ),
        Ffmpeg::BT2020NCL => (
            H273Matrix {
                kr: 0.2627,
                kb: 0.0593,
            },
            SourceMatrixCoefficients::Bt2020NonConstantLuminance,
        ),
        _ => return None,
    };
    Some(mapped)
}

pub(super) fn range(value: ffmpeg::color::Range) -> Option<(H273Range, SourceColorRange)> {
    match value {
        ffmpeg::color::Range::MPEG => Some((H273Range::Limited, SourceColorRange::Limited)),
        ffmpeg::color::Range::JPEG => Some((H273Range::Full, SourceColorRange::Full)),
        ffmpeg::color::Range::Unspecified => None,
    }
}

#[cfg(test)]
mod tests {
    use super::ChromaLocationResolution;
    use crate::plugin::{YuvChromaLocation, YuvChromaLocationSource};
    use ffmpeg_next::util::chroma::Location;

    #[test]
    fn frame_chroma_location_has_priority_and_is_recorded() {
        let resolved =
            ChromaLocationResolution::from_frame_and_decoder(Location::TopLeft, Location::Center)
                .applied()
                .expect("frame chroma location");
        assert_eq!(resolved.location(), YuvChromaLocation::TopLeft);
        assert_eq!(resolved.source(), YuvChromaLocationSource::Frame);
    }

    #[test]
    fn decoder_chroma_location_is_the_typed_fallback_and_is_recorded() {
        let resolution = ChromaLocationResolution::from_frame_and_decoder(
            Location::Unspecified,
            Location::Center,
        );
        let resolved = resolution.applied().expect("decoder chroma fallback");
        assert_eq!(resolved.location(), YuvChromaLocation::Center);
        assert_eq!(resolved.source(), YuvChromaLocationSource::Decoder);
        assert_eq!(
            resolution.for_frame(Location::Unspecified),
            Ok(YuvChromaLocation::Center)
        );
    }

    #[test]
    fn both_unspecified_locations_remain_an_explicit_failure() {
        let resolution = ChromaLocationResolution::from_frame_and_decoder(
            Location::Unspecified,
            Location::Unspecified,
        );
        assert!(resolution.applied().is_none());
        assert_eq!(
            resolution.for_frame(Location::Unspecified),
            Err("subsampled YUV frame and decoder both have unspecified chroma location")
        );
    }

    #[test]
    fn a_resolved_plan_fails_closed_if_current_frame_metadata_changes() {
        let frame_plan =
            ChromaLocationResolution::from_frame_and_decoder(Location::Left, Location::Center);
        assert_eq!(
            frame_plan.for_frame(Location::Center),
            Err("current frame chroma location differs from the verified frame plan")
        );

        let decoder_plan =
            ChromaLocationResolution::from_frame_and_decoder(Location::Unspecified, Location::Left);
        assert_eq!(
            decoder_plan.for_frame(Location::Center),
            Err("current frame chroma location contradicts the verified decoder fallback")
        );
        assert_eq!(
            decoder_plan.for_frame(Location::Unspecified),
            Ok(YuvChromaLocation::Left)
        );
    }
}
