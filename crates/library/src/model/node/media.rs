//! Persisted Media Node source identity and graph output contract.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

#[derive(Serialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct MediaContent {
    pub asset_id: Uuid,
    /// Exact media interface authored for this Node. Stream indices select a
    /// concrete stream or leave decoding on that output's default; they do
    /// not implicitly add or remove graph ports.
    pub output_selection: MediaOutputSelection,
    /// Primary visual/media stream as a zero-based global container index.
    pub stream_index: Option<usize>,
    /// Embedded audio override as a zero-based global container index.
    /// This is independent from the visual stream because they are distinct
    /// streams in a video container.
    pub audio_stream_index: Option<usize>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum MediaOutputSelection {
    Image,
    Audio,
    ImageAndAudio,
}

impl MediaContent {
    pub fn new(
        asset_id: Uuid,
        output_selection: MediaOutputSelection,
        stream_index: Option<usize>,
        audio_stream_index: Option<usize>,
    ) -> Result<Self, String> {
        let content = Self {
            asset_id,
            output_selection,
            stream_index,
            audio_stream_index,
        };
        content.validate()?;
        Ok(content)
    }

    pub const fn has_image_output(&self) -> bool {
        matches!(
            self.output_selection,
            MediaOutputSelection::Image | MediaOutputSelection::ImageAndAudio
        )
    }

    pub const fn has_audio_output(&self) -> bool {
        matches!(
            self.output_selection,
            MediaOutputSelection::Audio | MediaOutputSelection::ImageAndAudio
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        match self.output_selection {
            MediaOutputSelection::Image if self.audio_stream_index.is_some() => {
                Err("Image-only Media content cannot select an audio stream".to_string())
            }
            MediaOutputSelection::Audio if self.stream_index.is_some() => {
                Err("Audio-only Media content cannot select a visual stream".to_string())
            }
            MediaOutputSelection::Image
            | MediaOutputSelection::Audio
            | MediaOutputSelection::ImageAndAudio => Ok(()),
        }
    }
}

impl<'de> Deserialize<'de> for MediaContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            asset_id: Uuid,
            output_selection: MediaOutputSelection,
            stream_index: Option<usize>,
            #[serde(deserialize_with = "deserialize_required_audio_stream_index")]
            audio_stream_index: Option<usize>,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(
            wire.asset_id,
            wire.output_selection,
            wire.stream_index,
            wire.audio_stream_index,
        )
        .map_err(D::Error::custom)
    }
}

fn deserialize_required_audio_stream_index<'de, D>(
    deserializer: D,
) -> Result<Option<usize>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<usize>::deserialize(deserializer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_selection_is_independent_from_default_stream_indices() -> Result<(), String> {
        let asset_id = Uuid::new_v4();
        for (selection, image, audio) in [
            (MediaOutputSelection::Image, true, false),
            (MediaOutputSelection::Audio, false, true),
            (MediaOutputSelection::ImageAndAudio, true, true),
        ] {
            let content = MediaContent::new(asset_id, selection, None, None)?;
            assert_eq!(content.has_image_output(), image);
            assert_eq!(content.has_audio_output(), audio);
        }
        Ok(())
    }

    #[test]
    fn contradictory_stream_selection_is_rejected_on_create_and_load() {
        let asset_id = Uuid::new_v4();
        assert!(MediaContent::new(asset_id, MediaOutputSelection::Image, None, Some(2)).is_err());
        assert!(MediaContent::new(asset_id, MediaOutputSelection::Audio, Some(1), None).is_err());
        assert!(
            serde_json::from_value::<MediaContent>(serde_json::json!({
                "asset_id": asset_id,
                "output_selection": "audio",
                "stream_index": 1,
                "audio_stream_index": null
            }))
            .is_err()
        );
    }
}
